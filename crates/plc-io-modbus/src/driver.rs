//! Non-RT poll thread and the scan-facing snapshot bridge.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use plc_io::{
    BindingDirection, DoubleBuffer, DriverDiag, FieldGate, InputUpdate, IoDriver, IoError,
    OutputImage, OutputModuleState, PlcValue, RawType, RegisterType,
};
use plc_types::{OperatingMode, Quality};

use crate::address::{self, is_bit_table};
use crate::codec::Session;
use crate::convert;
use crate::validate::{ModbusPlan, PlannedPoint};

/// Scan-facing Modbus driver. `poll_inputs` / `apply_outputs` only copy snapshots.
pub struct ModbusBridge {
    shares: Vec<ModuleShare>,
    runtimes: Option<Vec<ModRt>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    running: bool,
    gate: Arc<FieldGate>,
    fails: Arc<AtomicU32>,
}

struct ModuleShare {
    inputs: Arc<DoubleBuffer>,
    input_slots: Vec<usize>,
    output_slots: Vec<usize>,
    mailbox: Arc<Mutex<Mail>>,
    status: Arc<Mutex<ModStatus>>,
    on_bad: plc_io::BadQualityPolicy,
    stale: Duration,
}

struct ModStatus {
    quality: Quality,
    last_ok: Option<Instant>,
}

#[derive(Clone)]
struct Mail {
    armed: bool,
    force_safe: bool,
    values: Vec<PlcValue>,
}

#[derive(Clone)]
struct Group {
    start: u16,
    qty: u16,
    members: Vec<usize>,
}

struct ModRt {
    host: String,
    port: u16,
    unit: u8,
    period: Duration,
    timeout: Duration,
    stale: Duration,
    on_bad: plc_io::BadQualityPolicy,
    points: Vec<PlannedPoint>,
    read_groups: Vec<Group>,
    coil_groups: Vec<Group>,
    reg_groups: Vec<Group>,
    bit_outs: Vec<usize>,
    inputs: Arc<DoubleBuffer>,
    last_inputs: Vec<PlcValue>,
    status: Arc<Mutex<ModStatus>>,
    mailbox: Arc<Mutex<Mail>>,
    stream: Option<TcpStream>,
    tid: u16,
    next_due: Instant,
}

enum Payload {
    Bits(Vec<u8>),
    Regs(Vec<u16>),
}

impl ModbusBridge {
    /// Build the bridge. The worker thread starts in [`IoDriver::start`].
    ///
    /// Input snapshots are Bad until the first successful poll.
    #[must_use]
    pub fn new(plan: ModbusPlan, gate: Arc<FieldGate>) -> Self {
        let mut shares = Vec::new();
        let mut runtimes = Vec::new();
        for module in plan.modules {
            let (share, rt) = split_module(module);
            shares.push(share);
            runtimes.push(rt);
        }
        Self {
            shares,
            runtimes: Some(runtimes),
            stop: Arc::new(AtomicBool::new(false)),
            join: None,
            running: false,
            gate,
            fails: Arc::new(AtomicU32::new(0)),
        }
    }

    fn stop_worker(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
        self.running = false;
    }
}

impl Drop for ModbusBridge {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

impl IoDriver for ModbusBridge {
    fn name(&self) -> &'static str {
        "modbus_tcp"
    }

    fn start(&mut self) -> Result<(), IoError> {
        if self.join.is_some() {
            self.running = true;
            return Ok(());
        }
        let rts = self
            .runtimes
            .take()
            .ok_or_else(|| IoError::NotReady("modbus worker was already started".into()))?;
        let stop = Arc::clone(&self.stop);
        let gate = Arc::clone(&self.gate);
        let fails = Arc::clone(&self.fails);
        self.join = Some(thread::spawn(move || {
            worker_loop(rts, &stop, &gate, &fails);
        }));
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.stop_worker();
    }

    fn poll_inputs(&mut self, out: &mut InputUpdate) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady("modbus_tcp".into()));
        }
        if self.gate.mode() != OperatingMode::Sim {
            self.overlay_inputs(out);
        }
        Ok(())
    }

    fn apply_outputs(&mut self, image: &OutputImage) -> Result<(), IoError> {
        if !self.running {
            return Err(IoError::NotReady("modbus_tcp".into()));
        }
        for share in &self.shares {
            let mut values = vec![PlcValue::Bool(false); share.output_slots.len()];
            for (pos, &slot) in share.output_slots.iter().enumerate() {
                if let Some(value) = image.values.get(slot).copied() {
                    values[pos] = value;
                }
            }
            let mut mail = share
                .mailbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            mail.armed = true;
            mail.force_safe = image.force_safe;
            mail.values = values;
        }
        Ok(())
    }

    fn diagnostics(&self) -> DriverDiag {
        let last_seq = self
            .shares
            .first()
            .map_or(0, |share| share.inputs.read(4).seq);
        DriverDiag {
            status: if self.running {
                "running".into()
            } else {
                "stopped".into()
            },
            fail_count: self.fails.load(Ordering::Relaxed),
            last_seq,
        }
    }

    fn fill_output_module_state(&self, into: &mut [OutputModuleState]) -> bool {
        for share in &self.shares {
            let quality = effective_quality(&share.status, share.stale);
            for &slot in &share.output_slots {
                if let Some(state) = into.get_mut(slot) {
                    state.quality = quality;
                    state.on_bad_quality = share.on_bad;
                }
            }
        }
        true
    }
}

impl ModbusBridge {
    fn overlay_inputs(&self, out: &mut InputUpdate) {
        for share in &self.shares {
            let snap = share.inputs.read(8);
            let module_q = effective_quality(&share.status, share.stale);
            for (i, &slot) in share.input_slots.iter().enumerate() {
                let Some(dest) = out.values.get_mut(slot) else {
                    continue;
                };
                let Some(dest_q) = out.quality.get_mut(slot) else {
                    continue;
                };
                if let Some(value) = snap.values.get(i).copied() {
                    *dest = value;
                }
                let point_q = snap.quality.get(i).copied().unwrap_or(Quality::Bad);
                *dest_q = if module_q.is_bad() {
                    Quality::Bad
                } else {
                    point_q
                };
            }
        }
    }
}

fn effective_quality(status: &Mutex<ModStatus>, stale: Duration) -> Quality {
    let st = status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if st.quality.is_bad() {
        return Quality::Bad;
    }
    match st.last_ok {
        Some(t) if t.elapsed() > stale => Quality::Bad,
        Some(_) => st.quality,
        None => Quality::Bad,
    }
}

fn split_module(module: crate::validate::PlannedModule) -> (ModuleShare, ModRt) {
    let n_in = module
        .points
        .iter()
        .filter(|p| p.input_pos.is_some())
        .count();
    let n_out = module
        .points
        .iter()
        .filter(|p| p.output_pos.is_some())
        .count();
    let mut input_slots = vec![0; n_in];
    let mut output_slots = vec![0; n_out];
    let mut defaults = vec![PlcValue::Bool(false); n_in];
    for point in &module.points {
        if let Some(pos) = point.input_pos {
            input_slots[pos] = point.slot;
            defaults[pos] = PlcValue::default_of(point.value_type);
        }
        if let Some(pos) = point.output_pos {
            output_slots[pos] = point.slot;
        }
    }
    let inputs = DoubleBuffer::new(n_in);
    inputs.publish(defaults.clone(), vec![Quality::Bad; n_in]);
    let status = Arc::new(Mutex::new(ModStatus {
        quality: Quality::Bad,
        last_ok: None,
    }));
    let mailbox = Arc::new(Mutex::new(Mail {
        armed: false,
        force_safe: false,
        values: vec![PlcValue::Bool(false); n_out],
    }));
    let points = module.points;
    let read_groups = read_groups(&points);
    let coil_groups = build_groups(
        &points,
        |p| p.direction == BindingDirection::Output && p.table == RegisterType::Coil,
        1968,
    );
    let reg_groups = build_groups(
        &points,
        |p| {
            p.direction == BindingDirection::Output
                && p.table == RegisterType::Holding
                && p.raw != RawType::Bool
        },
        123,
    );
    let bit_outs = points
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.direction == BindingDirection::Output
                && p.table == RegisterType::Holding
                && p.raw == RawType::Bool
        })
        .map(|(i, _)| i)
        .collect();
    let share = ModuleShare {
        inputs: Arc::clone(&inputs),
        input_slots,
        output_slots,
        mailbox: Arc::clone(&mailbox),
        status: Arc::clone(&status),
        on_bad: module.on_bad,
        stale: module.stale,
    };
    let rt = ModRt {
        host: module.host,
        port: module.port,
        unit: module.unit,
        period: Duration::from_millis(module.poll_ms),
        timeout: module.timeout,
        stale: module.stale,
        on_bad: module.on_bad,
        points,
        read_groups,
        coil_groups,
        reg_groups,
        bit_outs,
        inputs,
        last_inputs: defaults,
        status,
        mailbox,
        stream: None,
        tid: 1,
        next_due: Instant::now(),
    };
    (share, rt)
}

fn read_groups(points: &[PlannedPoint]) -> Vec<Group> {
    let tables = [
        RegisterType::Coil,
        RegisterType::Discrete,
        RegisterType::Holding,
        RegisterType::Input,
    ];
    let mut all = Vec::new();
    for table in tables {
        let max = if is_bit_table(table) { 2000 } else { 125 };
        all.extend(build_groups(
            points,
            |p| p.direction == BindingDirection::Input && p.table == table,
            max,
        ));
    }
    all
}

fn build_groups(
    points: &[PlannedPoint],
    pred: impl Fn(&PlannedPoint) -> bool,
    max_qty: u16,
) -> Vec<Group> {
    let mut idxs: Vec<usize> = (0..points.len()).filter(|&i| pred(&points[i])).collect();
    idxs.sort_by_key(|&i| points[i].pdu);
    let mut out = Vec::new();
    let mut cur: Option<Group> = None;
    for idx in idxs {
        let point = &points[idx];
        let start_new = match &cur {
            None => true,
            Some(group) => {
                let end = group.start.saturating_add(group.qty);
                let span = point
                    .pdu
                    .saturating_add(point.words)
                    .saturating_sub(group.start);
                !(point.pdu == end && span <= max_qty && span > 0)
            }
        };
        if start_new {
            if let Some(group) = cur.take() {
                out.push(group);
            }
            cur = Some(Group {
                start: point.pdu,
                qty: point.words,
                members: vec![idx],
            });
        } else if let Some(group) = cur.as_mut() {
            group.qty = point
                .pdu
                .saturating_add(point.words)
                .saturating_sub(group.start);
            group.members.push(idx);
        }
    }
    if let Some(group) = cur {
        out.push(group);
    }
    out
}

fn worker_loop(mut modules: Vec<ModRt>, stop: &AtomicBool, gate: &FieldGate, fails: &AtomicU32) {
    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        let mut soonest = now + Duration::from_secs(1);
        for module in &mut modules {
            if now >= module.next_due {
                module.poll(gate, fails);
                module.next_due = Instant::now() + module.period;
            }
            if module.next_due < soonest {
                soonest = module.next_due;
            }
        }
        sleep_until(stop, soonest);
    }
    for module in &mut modules {
        module.stream.take();
    }
}

fn sleep_until(stop: &AtomicBool, deadline: Instant) {
    while Instant::now() < deadline {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        thread::sleep(left.min(Duration::from_millis(10)));
    }
}

impl ModRt {
    fn poll(&mut self, gate: &FieldGate, fails: &AtomicU32) {
        if self.comms_stale() {
            self.mark_bad();
        }
        if self.stream.is_none() {
            match connect_to(&self.host, self.port, self.timeout) {
                Ok(stream) => self.stream = Some(stream),
                Err(_) => {
                    fails.fetch_add(1, Ordering::Relaxed);
                    self.mark_bad();
                    return;
                }
            }
        }
        if self.read_inputs().is_err() {
            fails.fetch_add(1, Ordering::Relaxed);
            self.stream = None;
            self.mark_bad();
            return;
        }
        self.mark_good();
        if gate.mode() == OperatingMode::Sim {
            return;
        }
        if self.write_outputs().is_err() {
            fails.fetch_add(1, Ordering::Relaxed);
            self.stream = None;
            self.mark_bad();
        }
    }

    fn comms_stale(&self) -> bool {
        let st = self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match st.last_ok {
            Some(t) => t.elapsed() > self.stale,
            None => false,
        }
    }

    fn mark_good(&self) {
        self.set_status(Quality::Good, true);
        self.publish(Quality::Good);
    }

    fn mark_bad(&self) {
        self.set_status(Quality::Bad, false);
        self.publish(Quality::Bad);
    }

    fn set_status(&self, quality: Quality, success: bool) {
        let mut st = self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        st.quality = quality;
        if success {
            st.last_ok = Some(Instant::now());
        }
    }

    fn publish(&self, quality: Quality) {
        if self.last_inputs.is_empty() {
            return;
        }
        self.inputs.publish(
            self.last_inputs.clone(),
            vec![quality; self.last_inputs.len()],
        );
    }

    fn read_inputs(&mut self) -> Result<(), String> {
        let groups = self.read_groups.clone();
        for group in &groups {
            let table = self.points[group.members[0]].table;
            let fc = address::read_fc(table);
            let mut stream = self.stream.take().ok_or_else(|| "no socket".to_string())?;
            let mut tid = self.tid;
            let read = {
                let mut session = Session {
                    stream: &mut stream,
                    unit: self.unit,
                    tid: &mut tid,
                };
                if is_bit_table(table) {
                    session
                        .read_bits(fc, group.start, group.qty)
                        .map(Payload::Bits)
                } else {
                    session
                        .read_regs(fc, group.start, group.qty)
                        .map(Payload::Regs)
                }
            };
            self.tid = tid;
            match read {
                Ok(data) => {
                    if let Err(err) = self.store_group(group, &data) {
                        self.stream = None;
                        return Err(err);
                    }
                    self.stream = Some(stream);
                }
                Err(err) => {
                    self.stream = None;
                    return Err(err.text());
                }
            }
        }
        Ok(())
    }

    fn store_group(&mut self, group: &Group, data: &Payload) -> Result<(), String> {
        for &index in &group.members {
            let point = &self.points[index];
            let rel = usize::from(point.pdu.saturating_sub(group.start));
            let value = match data {
                Payload::Bits(bytes) => {
                    let on = bytes
                        .get(rel / 8)
                        .is_some_and(|byte| (byte >> (rel % 8)) & 1 == 1);
                    convert::decode_number(if on { 1.0 } else { 0.0 }, point)
                }
                Payload::Regs(regs) => {
                    let words = usize::from(point.words);
                    if rel + words > regs.len() {
                        return Err("short register block".into());
                    }
                    convert::decode_regs(&regs[rel..rel + words], point)
                }
            };
            if let Some(pos) = point.input_pos {
                if let Some(slot) = self.last_inputs.get_mut(pos) {
                    *slot = value;
                }
            }
        }
        Ok(())
    }

    fn write_outputs(&mut self) -> Result<(), String> {
        let mail = {
            let guard = self
                .mailbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !guard.armed {
                return Ok(());
            }
            guard.clone()
        };
        let bad = self
            .status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .quality
            .is_bad();
        if bad && self.on_bad == plc_io::BadQualityPolicy::HoldLast {
            return Ok(());
        }
        let force = mail.force_safe || (bad && self.on_bad == plc_io::BadQualityPolicy::ForceSafe);
        let mut stream = self.stream.take().ok_or_else(|| "no socket".to_string())?;
        let mut tid = self.tid;
        let ctx = WriteCtx {
            points: &self.points,
            coil_groups: &self.coil_groups,
            reg_groups: &self.reg_groups,
            bit_outs: &self.bit_outs,
            mail: &mail,
            force,
        };
        let result = write_with(&ctx, &mut stream, self.unit, &mut tid);
        self.tid = tid;
        if result.is_ok() {
            self.stream = Some(stream);
        }
        result
    }
}

struct WriteCtx<'a> {
    points: &'a [PlannedPoint],
    coil_groups: &'a [Group],
    reg_groups: &'a [Group],
    bit_outs: &'a [usize],
    mail: &'a Mail,
    force: bool,
}

fn write_with(
    ctx: &WriteCtx<'_>,
    stream: &mut TcpStream,
    unit: u8,
    tid: &mut u16,
) -> Result<(), String> {
    let mut session = Session { stream, unit, tid };
    for group in ctx.coil_groups {
        let mut bits = Vec::with_capacity(group.members.len());
        for &index in &group.members {
            let point = &ctx.points[index];
            bits.push(convert::coil_on(
                selected(point, ctx.mail, ctx.force),
                point,
            ));
        }
        let result = if bits.len() == 1 {
            session.write_coil(group.start, bits[0])
        } else {
            session.write_coils(group.start, &bits)
        };
        result.map_err(|err| err.text())?;
    }
    for group in ctx.reg_groups {
        let mut regs = vec![0u16; usize::from(group.qty)];
        for &index in &group.members {
            let point = &ctx.points[index];
            let words = convert::value_to_words(selected(point, ctx.mail, ctx.force), point);
            let off = usize::from(point.pdu.saturating_sub(group.start));
            for (i, word) in words.iter().enumerate() {
                if let Some(slot) = regs.get_mut(off + i) {
                    *slot = *word;
                }
            }
        }
        let result = if regs.len() == 1 {
            session.write_reg(group.start, regs[0])
        } else if !regs.is_empty() {
            session.write_regs(group.start, &regs)
        } else {
            Ok(())
        };
        result.map_err(|err| err.text())?;
    }
    for &index in ctx.bit_outs {
        let point = &ctx.points[index];
        let regs = session
            .read_regs(3, point.pdu, 1)
            .map_err(|err| err.text())?;
        let mut reg = regs.first().copied().unwrap_or(0);
        let bit = point.bit.unwrap_or(0);
        if convert::coil_on(selected(point, ctx.mail, ctx.force), point) {
            reg |= 1 << bit;
        } else {
            reg &= !(1 << bit);
        }
        session
            .write_reg(point.pdu, reg)
            .map_err(|err| err.text())?;
    }
    Ok(())
}

fn selected(point: &PlannedPoint, mail: &Mail, force: bool) -> PlcValue {
    if force {
        point.safe
    } else {
        point
            .output_pos
            .and_then(|pos| mail.values.get(pos).copied())
            .unwrap_or(point.safe)
    }
}

fn connect_to(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, String> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|err| err.to_string())?;
    let mut last = format!("no addresses for {host}:{port}");
    let mut any = false;
    for addr in addrs {
        any = true;
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|e| e.to_string())?;
                stream.set_nodelay(true).map_err(|e| e.to_string())?;
                return Ok(stream);
            }
            Err(err) => last = err.to_string(),
        }
    }
    if !any {
        last = format!("no addresses for {host}:{port}");
    }
    Err(last)
}

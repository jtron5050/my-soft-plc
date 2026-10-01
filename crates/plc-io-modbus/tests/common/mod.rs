//! Loopback Modbus TCP peer for driver tests.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct WriteRec {
    pub fc: u8,
    pub addr: u16,
    pub coils: Vec<bool>,
    pub regs: Vec<u16>,
}

pub struct Mock {
    pub holdings: Mutex<Vec<u16>>,
    pub inputs: Mutex<Vec<u16>>,
    pub coils: Mutex<Vec<bool>>,
    pub discrete: Mutex<Vec<bool>>,
    pub writes: Mutex<Vec<WriteRec>>,
    pub stall_conn: AtomicBool,
    pub hang_reads: AtomicBool,
    pub good_reads: AtomicU32,
    pub read_count: AtomicU32,
    pub wrong_once: AtomicBool,
    pub always_wrong: AtomicBool,
    pub exception: AtomicBool,
    pub accepts: AtomicU32,
    pub stop: Arc<AtomicBool>,
}

impl Mock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            holdings: Mutex::new(vec![0; 64]),
            inputs: Mutex::new(vec![0; 64]),
            coils: Mutex::new(vec![false; 64]),
            discrete: Mutex::new(vec![false; 64]),
            writes: Mutex::new(Vec::new()),
            stall_conn: AtomicBool::new(false),
            hang_reads: AtomicBool::new(false),
            good_reads: AtomicU32::new(u32::MAX),
            read_count: AtomicU32::new(0),
            wrong_once: AtomicBool::new(false),
            always_wrong: AtomicBool::new(false),
            exception: AtomicBool::new(false),
            accepts: AtomicU32::new(0),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }
}

pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    joins: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Server {
    pub fn spawn(state: Arc<Mock>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        listener.set_nonblocking(true).expect("nonblocking");
        let stop = Arc::clone(&state.stop);
        let joins = Arc::new(Mutex::new(Vec::new()));
        let joins_thr = Arc::clone(&joins);
        let stop_thr = Arc::clone(&stop);
        let accept = thread::spawn(move || {
            while !stop_thr.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((sock, _)) => {
                        let state = Arc::clone(&state);
                        let handle = thread::spawn(move || session(sock, &state));
                        joins_thr.lock().expect("joins").push(handle);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            stop,
            accept: Some(accept),
            joins,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
        let joins = std::mem::take(&mut *self.joins.lock().expect("joins"));
        for handle in joins {
            let _ = handle.join();
        }
    }
}

fn session(mut sock: TcpStream, state: &Mock) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = sock.set_nodelay(true);
    state.accepts.fetch_add(1, Ordering::Relaxed);
    if state.stall_conn.load(Ordering::Acquire) {
        while !state.stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    loop {
        if state.stop.load(Ordering::Acquire) {
            return;
        }
        let Some((tid, unit, pdu)) = read_adu(&mut sock) else {
            return;
        };
        if pdu.is_empty() {
            return;
        }
        let fc = pdu[0];
        if fc <= 4 {
            let n = state.read_count.fetch_add(1, Ordering::Relaxed);
            let hang = state.hang_reads.load(Ordering::Acquire)
                || n >= state.good_reads.load(Ordering::Acquire);
            if hang {
                while !state.stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(10));
                }
                return;
            }
        }
        let resp = handle_pdu(state, &pdu);
        if state.always_wrong.load(Ordering::Acquire) {
            let _ = write_adu(&mut sock, tid.wrapping_add(1), unit, &resp);
            let _ = write_adu(&mut sock, tid.wrapping_add(2), unit, &resp);
            continue;
        }
        if state.wrong_once.swap(false, Ordering::AcqRel) {
            let _ = write_adu(&mut sock, tid.wrapping_add(1), unit, &resp);
        }
        if write_adu(&mut sock, tid, unit, &resp).is_err() {
            return;
        }
    }
}

fn handle_pdu(state: &Mock, pdu: &[u8]) -> Vec<u8> {
    let fc = pdu[0];
    if state.exception.load(Ordering::Acquire) && fc <= 4 {
        return vec![fc | 0x80, 0x02];
    }
    if pdu.len() < 5 && matches!(fc, 1..=6) {
        return vec![fc | 0x80, 0x03];
    }
    match fc {
        1 => read_bits(state, fc, pdu, true),
        2 => read_bits(state, fc, pdu, false),
        3 => read_regs(state, fc, pdu, true),
        4 => read_regs(state, fc, pdu, false),
        5 => write_one_coil(state, pdu),
        6 => write_one_reg(state, pdu),
        15 => write_many_coils(state, pdu),
        16 => write_many_regs(state, pdu),
        _ => vec![fc | 0x80, 0x01],
    }
}

fn addr_qty(pdu: &[u8]) -> (usize, usize) {
    let start = usize::from(u16::from_be_bytes([pdu[1], pdu[2]]));
    let qty = usize::from(u16::from_be_bytes([pdu[3], pdu[4]]));
    (start, qty)
}

fn read_regs(state: &Mock, fc: u8, pdu: &[u8], holding: bool) -> Vec<u8> {
    let (start, qty) = addr_qty(pdu);
    let bank = if holding {
        state.holdings.lock().expect("hold")
    } else {
        state.inputs.lock().expect("in")
    };
    if qty == 0 || start + qty > bank.len() {
        return vec![fc | 0x80, 0x02];
    }
    let mut resp = vec![fc, u8::try_from(qty * 2).unwrap_or(255)];
    for i in 0..qty {
        resp.extend(bank[start + i].to_be_bytes());
    }
    resp
}

fn read_bits(state: &Mock, fc: u8, pdu: &[u8], coils: bool) -> Vec<u8> {
    let (start, qty) = addr_qty(pdu);
    let bank = if coils {
        state.coils.lock().expect("coils")
    } else {
        state.discrete.lock().expect("disc")
    };
    if qty == 0 || start + qty > bank.len() {
        return vec![fc | 0x80, 0x02];
    }
    let nbytes = qty.div_ceil(8);
    let mut data = vec![0u8; nbytes];
    for i in 0..qty {
        if bank[start + i] {
            data[i / 8] |= 1 << (i % 8);
        }
    }
    let mut resp = vec![fc, u8::try_from(nbytes).unwrap_or(255)];
    resp.extend(data);
    resp
}

fn write_one_coil(state: &Mock, pdu: &[u8]) -> Vec<u8> {
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]);
    let raw = u16::from_be_bytes([pdu[3], pdu[4]]);
    let on = raw == 0xFF00;
    {
        let mut coils = state.coils.lock().expect("coils");
        if usize::from(addr) >= coils.len() {
            return vec![5 | 0x80, 0x02];
        }
        coils[usize::from(addr)] = on;
    }
    state.writes.lock().expect("w").push(WriteRec {
        fc: 5,
        addr,
        coils: vec![on],
        regs: Vec::new(),
    });
    pdu.to_vec()
}

fn write_one_reg(state: &Mock, pdu: &[u8]) -> Vec<u8> {
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]);
    let value = u16::from_be_bytes([pdu[3], pdu[4]]);
    {
        let mut regs = state.holdings.lock().expect("hold");
        if usize::from(addr) >= regs.len() {
            return vec![6 | 0x80, 0x02];
        }
        regs[usize::from(addr)] = value;
    }
    state.writes.lock().expect("w").push(WriteRec {
        fc: 6,
        addr,
        coils: Vec::new(),
        regs: vec![value],
    });
    pdu.to_vec()
}

fn write_many_coils(state: &Mock, pdu: &[u8]) -> Vec<u8> {
    if pdu.len() < 6 {
        return vec![15 | 0x80, 0x03];
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]);
    let qty = usize::from(u16::from_be_bytes([pdu[3], pdu[4]]));
    let mut bits = Vec::with_capacity(qty);
    for i in 0..qty {
        let byte = pdu.get(6 + i / 8).copied().unwrap_or(0);
        bits.push((byte >> (i % 8)) & 1 == 1);
    }
    {
        let mut coils = state.coils.lock().expect("coils");
        if usize::from(addr) + qty > coils.len() {
            return vec![15 | 0x80, 0x02];
        }
        for (i, on) in bits.iter().enumerate() {
            coils[usize::from(addr) + i] = *on;
        }
    }
    state.writes.lock().expect("w").push(WriteRec {
        fc: 15,
        addr,
        coils: bits,
        regs: Vec::new(),
    });
    vec![15, pdu[1], pdu[2], pdu[3], pdu[4]]
}

fn write_many_regs(state: &Mock, pdu: &[u8]) -> Vec<u8> {
    if pdu.len() < 6 {
        return vec![16 | 0x80, 0x03];
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]);
    let qty = usize::from(u16::from_be_bytes([pdu[3], pdu[4]]));
    let mut regs = Vec::with_capacity(qty);
    for i in 0..qty {
        let o = 6 + i * 2;
        if o + 1 >= pdu.len() {
            return vec![16 | 0x80, 0x03];
        }
        regs.push(u16::from_be_bytes([pdu[o], pdu[o + 1]]));
    }
    {
        let mut bank = state.holdings.lock().expect("hold");
        if usize::from(addr) + qty > bank.len() {
            return vec![16 | 0x80, 0x02];
        }
        for (i, value) in regs.iter().enumerate() {
            bank[usize::from(addr) + i] = *value;
        }
    }
    state.writes.lock().expect("w").push(WriteRec {
        fc: 16,
        addr,
        coils: Vec::new(),
        regs,
    });
    vec![16, pdu[1], pdu[2], pdu[3], pdu[4]]
}

fn read_adu(sock: &mut TcpStream) -> Option<(u16, u8, Vec<u8>)> {
    let mut prefix = [0u8; 6];
    sock.read_exact(&mut prefix).ok()?;
    let tid = u16::from_be_bytes([prefix[0], prefix[1]]);
    let len = usize::from(u16::from_be_bytes([prefix[4], prefix[5]]));
    if !(2..=254).contains(&len) {
        return None;
    }
    let mut rest = vec![0u8; len];
    sock.read_exact(&mut rest).ok()?;
    let unit = rest[0];
    Some((tid, unit, rest[1..].to_vec()))
}

fn write_adu(sock: &mut TcpStream, tid: u16, unit: u8, pdu: &[u8]) -> std::io::Result<()> {
    let len = u16::try_from(1 + pdu.len()).unwrap_or(0);
    let mut buf = Vec::with_capacity(6 + pdu.len());
    buf.extend(tid.to_be_bytes());
    buf.extend(0u16.to_be_bytes());
    buf.extend(len.to_be_bytes());
    buf.push(unit);
    buf.extend(pdu);
    sock.write_all(&buf)
}

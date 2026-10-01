//! Modbus TCP MBAP client (function codes 1, 2, 3, 4, 5, 6, 15, 16).

use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Debug)]
pub(crate) enum Fail {
    /// Response transaction id did not match. The ADU was consumed.
    Mismatch,
    /// Timeout, exception, or framing error.
    Other(String),
}

pub(crate) struct Session<'a> {
    pub stream: &'a mut TcpStream,
    pub unit: u8,
    pub tid: &'a mut u16,
}

impl Session<'_> {
    pub fn read_bits(&mut self, fc: u8, start: u16, qty: u16) -> Result<Vec<u8>, Fail> {
        let pdu = self.transact(&read_pdu(fc, start, qty))?;
        let count = usize::from(pdu.get(1).copied().unwrap_or(0));
        let expect = (usize::from(qty) + 7) / 8;
        if count != expect || pdu.len() < 2 + count {
            return Err(Fail::Other("short bit response".into()));
        }
        Ok(pdu[2..2 + count].to_vec())
    }

    pub fn read_regs(&mut self, fc: u8, start: u16, qty: u16) -> Result<Vec<u16>, Fail> {
        let pdu = self.transact(&read_pdu(fc, start, qty))?;
        let count = usize::from(pdu.get(1).copied().unwrap_or(0));
        if count != usize::from(qty) * 2 || pdu.len() < 2 + count {
            return Err(Fail::Other("short register response".into()));
        }
        let mut regs = Vec::with_capacity(usize::from(qty));
        for i in 0..usize::from(qty) {
            let o = 2 + i * 2;
            regs.push(u16::from_be_bytes([pdu[o], pdu[o + 1]]));
        }
        Ok(regs)
    }

    pub fn write_coil(&mut self, addr: u16, on: bool) -> Result<(), Fail> {
        let value: u16 = if on { 0xFF00 } else { 0 };
        let mut pdu = [0u8; 5];
        pdu[0] = 5;
        pdu[1..3].copy_from_slice(&addr.to_be_bytes());
        pdu[3..5].copy_from_slice(&value.to_be_bytes());
        self.transact(&pdu)?;
        Ok(())
    }

    pub fn write_reg(&mut self, addr: u16, value: u16) -> Result<(), Fail> {
        let mut pdu = [0u8; 5];
        pdu[0] = 6;
        pdu[1..3].copy_from_slice(&addr.to_be_bytes());
        pdu[3..5].copy_from_slice(&value.to_be_bytes());
        self.transact(&pdu)?;
        Ok(())
    }

    pub fn write_coils(&mut self, addr: u16, bits: &[bool]) -> Result<(), Fail> {
        let nbytes = bits.len().div_ceil(8);
        let mut pdu = Vec::with_capacity(6 + nbytes);
        pdu.push(15);
        pdu.extend(addr.to_be_bytes());
        pdu.extend(u16::try_from(bits.len()).unwrap_or(0).to_be_bytes());
        pdu.push(u8::try_from(nbytes).unwrap_or(255));
        let mut data = vec![0u8; nbytes];
        for (i, on) in bits.iter().enumerate() {
            if *on {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        pdu.extend(data);
        self.transact(&pdu)?;
        Ok(())
    }

    pub fn write_regs(&mut self, addr: u16, regs: &[u16]) -> Result<(), Fail> {
        let mut pdu = Vec::with_capacity(6 + regs.len() * 2);
        pdu.push(16);
        pdu.extend(addr.to_be_bytes());
        pdu.extend(u16::try_from(regs.len()).unwrap_or(0).to_be_bytes());
        pdu.push(u8::try_from(regs.len() * 2).unwrap_or(255));
        for reg in regs {
            pdu.extend(reg.to_be_bytes());
        }
        self.transact(&pdu)?;
        Ok(())
    }

    fn transact(&mut self, request: &[u8]) -> Result<Vec<u8>, Fail> {
        let tid = *self.tid;
        *self.tid = self.tid.wrapping_add(1);
        self.send(tid, request)?;
        let pdu = match self.recv_tid(tid) {
            Err(Fail::Mismatch) => self.recv_tid(tid)?,
            other => other?,
        };
        let fc = request.first().copied().unwrap_or(0);
        match pdu.first().copied() {
            Some(got) if got == fc | 0x80 => {
                let code = pdu.get(1).copied().unwrap_or(0);
                Err(Fail::Other(format!("modbus exception {code}")))
            }
            Some(got) if got == fc => Ok(pdu),
            _ => Err(Fail::Other("unexpected function code".into())),
        }
    }

    fn send(&mut self, tid: u16, pdu: &[u8]) -> Result<(), Fail> {
        let len = u16::try_from(1 + pdu.len()).unwrap_or(0);
        let mut buf = Vec::with_capacity(6 + usize::from(len));
        buf.extend(tid.to_be_bytes());
        buf.extend(0u16.to_be_bytes());
        buf.extend(len.to_be_bytes());
        buf.push(self.unit);
        buf.extend(pdu);
        self.stream.write_all(&buf).map_err(|err| io_fail(&err))
    }

    fn recv_tid(&mut self, expect: u16) -> Result<Vec<u8>, Fail> {
        let mut prefix = [0u8; 6];
        self.stream
            .read_exact(&mut prefix)
            .map_err(|err| io_fail(&err))?;
        let tid = u16::from_be_bytes([prefix[0], prefix[1]]);
        let proto = u16::from_be_bytes([prefix[2], prefix[3]]);
        let len = usize::from(u16::from_be_bytes([prefix[4], prefix[5]]));
        if proto != 0 {
            return Err(Fail::Other("protocol id".into()));
        }
        if !(2..=254).contains(&len) {
            return Err(Fail::Other(format!("bad mbap length {len}")));
        }
        let mut rest = vec![0u8; len];
        self.stream
            .read_exact(&mut rest)
            .map_err(|err| io_fail(&err))?;
        if tid != expect {
            return Err(Fail::Mismatch);
        }
        if rest[0] != self.unit {
            return Err(Fail::Other("unit id mismatch".into()));
        }
        Ok(rest[1..].to_vec())
    }
}

fn read_pdu(fc: u8, start: u16, qty: u16) -> [u8; 5] {
    let mut pdu = [0u8; 5];
    pdu[0] = fc;
    pdu[1..3].copy_from_slice(&start.to_be_bytes());
    pdu[3..5].copy_from_slice(&qty.to_be_bytes());
    pdu
}

fn io_fail(err: &std::io::Error) -> Fail {
    Fail::Other(err.to_string())
}

impl Fail {
    pub fn text(&self) -> String {
        match self {
            Self::Mismatch => "transaction id mismatch".into(),
            Self::Other(s) => s.clone(),
        }
    }
}

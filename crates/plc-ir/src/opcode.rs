//! Opcode catalog and primitive FB ids (Appendix A.4 / A.5).

/// IR v0.1 opcodes (bits 31–24 of each instruction word).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    /// No operation.
    Nop = 0x00,
    /// End of task entry.
    Halt = 0x01,
    /// Push DINT immediate (following word).
    PushIDint = 0x02,
    /// Push REAL immediate (following word).
    PushIReal = 0x03,
    /// Push BOOL immediate in payload.
    PushIBool = 0x04,
    /// Push TIME immediate (following word, ms).
    PushTime = 0x05,
    /// Load from data segment.
    LdData = 0x10,
    /// Store to data segment.
    StData = 0x11,
    /// Load retain.
    LdRetain = 0x12,
    /// Store retain.
    StRetain = 0x13,
    /// Load input image slot.
    LdI = 0x14,
    /// Store output image slot.
    StQ = 0x15,
    /// Load output image slot.
    LdQ = 0x16,
    /// Load input quality Good? as BOOL.
    LdIq = 0x17,
    /// Add.
    Add = 0x20,
    /// Subtract.
    Sub = 0x21,
    /// Multiply.
    Mul = 0x22,
    /// Divide.
    Div = 0x23,
    /// Negate.
    Neg = 0x24,
    /// And.
    And = 0x28,
    /// Or.
    Or = 0x29,
    /// Xor.
    Xor = 0x2A,
    /// Not.
    Not = 0x2B,
    /// Equal.
    Eq = 0x30,
    /// Not equal.
    Ne = 0x31,
    /// Less than.
    Lt = 0x32,
    /// Less or equal.
    Le = 0x33,
    /// Greater than.
    Gt = 0x34,
    /// Greater or equal.
    Ge = 0x35,
    /// Unconditional jump.
    Jmp = 0x40,
    /// Jump if true.
    JmpIf = 0x41,
    /// Jump if false.
    JmpIfNot = 0x42,
    /// Call FB.
    CallFb = 0x50,
    /// Return from user FB.
    Ret = 0x51,
    /// Convert type.
    Conv = 0x60,
}

impl Opcode {
    /// Decode from opcode byte.
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Nop),
            0x01 => Some(Self::Halt),
            0x02 => Some(Self::PushIDint),
            0x03 => Some(Self::PushIReal),
            0x04 => Some(Self::PushIBool),
            0x05 => Some(Self::PushTime),
            0x10 => Some(Self::LdData),
            0x11 => Some(Self::StData),
            0x12 => Some(Self::LdRetain),
            0x13 => Some(Self::StRetain),
            0x14 => Some(Self::LdI),
            0x15 => Some(Self::StQ),
            0x16 => Some(Self::LdQ),
            0x17 => Some(Self::LdIq),
            0x20 => Some(Self::Add),
            0x21 => Some(Self::Sub),
            0x22 => Some(Self::Mul),
            0x23 => Some(Self::Div),
            0x24 => Some(Self::Neg),
            0x28 => Some(Self::And),
            0x29 => Some(Self::Or),
            0x2A => Some(Self::Xor),
            0x2B => Some(Self::Not),
            0x30 => Some(Self::Eq),
            0x31 => Some(Self::Ne),
            0x32 => Some(Self::Lt),
            0x33 => Some(Self::Le),
            0x34 => Some(Self::Gt),
            0x35 => Some(Self::Ge),
            0x40 => Some(Self::Jmp),
            0x41 => Some(Self::JmpIf),
            0x42 => Some(Self::JmpIfNot),
            0x50 => Some(Self::CallFb),
            0x51 => Some(Self::Ret),
            0x60 => Some(Self::Conv),
            _ => None,
        }
    }

    /// Mnemonic used in `spasm`.
    #[must_use]
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Self::Nop => "NOP",
            Self::Halt => "HALT",
            Self::PushIDint => "PUSHI_DINT",
            Self::PushIReal => "PUSHI_REAL",
            Self::PushIBool => "PUSHI_BOOL",
            Self::PushTime => "PUSH_TIME",
            Self::LdData => "LD_DATA",
            Self::StData => "ST_DATA",
            Self::LdRetain => "LD_RETAIN",
            Self::StRetain => "ST_RETAIN",
            Self::LdI => "LD_I",
            Self::StQ => "ST_Q",
            Self::LdQ => "LD_Q",
            Self::LdIq => "LD_IQ",
            Self::Add => "ADD",
            Self::Sub => "SUB",
            Self::Mul => "MUL",
            Self::Div => "DIV",
            Self::Neg => "NEG",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Xor => "XOR",
            Self::Not => "NOT",
            Self::Eq => "EQ",
            Self::Ne => "NE",
            Self::Lt => "LT",
            Self::Le => "LE",
            Self::Gt => "GT",
            Self::Ge => "GE",
            Self::Jmp => "JMP",
            Self::JmpIf => "JMP_IF",
            Self::JmpIfNot => "JMP_IF_NOT",
            Self::CallFb => "CALL_FB",
            Self::Ret => "RET",
            Self::Conv => "CONV",
        }
    }

    /// Parse mnemonic (case-insensitive).
    #[must_use]
    pub fn from_mnemonic(s: &str) -> Option<Self> {
        let u = s.to_ascii_uppercase();
        match u.as_str() {
            "NOP" => Some(Self::Nop),
            "HALT" => Some(Self::Halt),
            "PUSHI_DINT" => Some(Self::PushIDint),
            "PUSHI_REAL" => Some(Self::PushIReal),
            "PUSHI_BOOL" => Some(Self::PushIBool),
            "PUSH_TIME" => Some(Self::PushTime),
            "LD_DATA" => Some(Self::LdData),
            "ST_DATA" => Some(Self::StData),
            "LD_RETAIN" => Some(Self::LdRetain),
            "ST_RETAIN" => Some(Self::StRetain),
            "LD_I" => Some(Self::LdI),
            "ST_Q" => Some(Self::StQ),
            "LD_Q" => Some(Self::LdQ),
            "LD_IQ" => Some(Self::LdIq),
            "ADD" => Some(Self::Add),
            "SUB" => Some(Self::Sub),
            "MUL" => Some(Self::Mul),
            "DIV" => Some(Self::Div),
            "NEG" => Some(Self::Neg),
            "AND" => Some(Self::And),
            "OR" => Some(Self::Or),
            "XOR" => Some(Self::Xor),
            "NOT" => Some(Self::Not),
            "EQ" => Some(Self::Eq),
            "NE" => Some(Self::Ne),
            "LT" => Some(Self::Lt),
            "LE" => Some(Self::Le),
            "GT" => Some(Self::Gt),
            "GE" => Some(Self::Ge),
            "JMP" => Some(Self::Jmp),
            "JMP_IF" => Some(Self::JmpIf),
            "JMP_IF_NOT" => Some(Self::JmpIfNot),
            "CALL_FB" => Some(Self::CallFb),
            "RET" => Some(Self::Ret),
            "CONV" => Some(Self::Conv),
            _ => None,
        }
    }
}

/// Built-in primitive function block identifiers for `CALL_FB`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveId {
    /// On-delay timer.
    Ton = 1,
    /// Off-delay timer.
    Tof = 2,
    /// Pulse timer.
    Tp = 3,
    /// Count up.
    Ctu = 4,
    /// Count down.
    Ctd = 5,
    /// Reset-dominant latch.
    Rs = 6,
    /// Set-dominant latch.
    Sr = 7,
    /// Rising edge.
    RTrig = 8,
    /// Falling edge.
    FTrig = 9,
    /// PID controller.
    Pid = 10,
}

impl PrimitiveId {
    /// Parse primitive name (e.g. `TON`).
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "TON" => Some(Self::Ton),
            "TOF" => Some(Self::Tof),
            "TP" => Some(Self::Tp),
            "CTU" => Some(Self::Ctu),
            "CTD" => Some(Self::Ctd),
            "RS" => Some(Self::Rs),
            "SR" => Some(Self::Sr),
            "R_TRIG" | "RTRIG" => Some(Self::RTrig),
            "F_TRIG" | "FTRIG" => Some(Self::FTrig),
            "PID" => Some(Self::Pid),
            _ => None,
        }
    }

    /// Canonical name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ton => "TON",
            Self::Tof => "TOF",
            Self::Tp => "TP",
            Self::Ctu => "CTU",
            Self::Ctd => "CTD",
            Self::Rs => "RS",
            Self::Sr => "SR",
            Self::RTrig => "R_TRIG",
            Self::FTrig => "F_TRIG",
            Self::Pid => "PID",
        }
    }

    /// Stack inputs consumed (declaration order).
    #[must_use]
    pub const fn input_count(self) -> u8 {
        match self {
            Self::Ton | Self::Tof | Self::Tp => 2, // IN, PT
            Self::Ctu | Self::Ctd => 2,            // CU/CD, PV (simplified)
            Self::Rs | Self::Sr => 2,              // S, R
            Self::RTrig | Self::FTrig => 1,        // CLK
            Self::Pid => 3,                        // PV, SP, enable (simplified)
        }
    }

    /// Stack outputs produced.
    #[must_use]
    pub const fn output_count(self) -> u8 {
        match self {
            Self::Ton | Self::Tof | Self::Tp => 2, // Q, ET
            Self::Ctu | Self::Ctd => 2,            // Q, CV
            Self::Rs | Self::Sr => 1,              // Q
            Self::RTrig | Self::FTrig => 1,        // Q
            Self::Pid => 1,                        // OUT
        }
    }
}

//! 5-digit Modicon reference decode.

use plc_io::RegisterType;

/// Decode `register` to a table and 0-based PDU address.
pub(crate) fn decode(
    register: u32,
    hint: Option<RegisterType>,
) -> Result<(RegisterType, u16), String> {
    let (table, pdu) = match register {
        1..=9_999 => (RegisterType::Coil, register - 1),
        10_001..=19_999 => (RegisterType::Discrete, register - 10_001),
        30_001..=39_999 => (RegisterType::Input, register - 30_001),
        40_001..=49_999 => (RegisterType::Holding, register - 40_001),
        _ => {
            return Err(format!(
                "register {register} is not a 5-digit Modicon reference"
            ));
        }
    };
    if let Some(hint) = hint {
        if hint != table {
            return Err(format!(
                "register {register} is {}, not {}",
                table_name(table),
                table_name(hint)
            ));
        }
    }
    Ok((table, u16::try_from(pdu).unwrap_or(0)))
}

pub(crate) fn table_name(table: RegisterType) -> &'static str {
    match table {
        RegisterType::Coil => "coil",
        RegisterType::Discrete => "discrete",
        RegisterType::Holding => "holding",
        RegisterType::Input => "input",
    }
}

pub(crate) fn is_bit_table(table: RegisterType) -> bool {
    matches!(table, RegisterType::Coil | RegisterType::Discrete)
}

pub(crate) fn read_fc(table: RegisterType) -> u8 {
    match table {
        RegisterType::Coil => 1,
        RegisterType::Discrete => 2,
        RegisterType::Holding => 3,
        RegisterType::Input => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modicon_bases() {
        assert_eq!(decode(1, None).unwrap(), (RegisterType::Coil, 0));
        assert_eq!(decode(2, Some(RegisterType::Coil)).unwrap().1, 1);
        assert_eq!(decode(10_001, None).unwrap(), (RegisterType::Discrete, 0));
        assert_eq!(decode(30_001, None).unwrap(), (RegisterType::Input, 0));
        assert_eq!(
            decode(40_001, Some(RegisterType::Holding)).unwrap(),
            (RegisterType::Holding, 0)
        );
        assert_eq!(decode(40_101, None).unwrap().1, 100);
    }

    #[test]
    fn rejects_mismatch_and_out_of_range() {
        assert!(decode(40_001, Some(RegisterType::Coil))
            .unwrap_err()
            .contains("not coil"));
        assert!(decode(0, None).unwrap_err().contains("5-digit"));
        assert!(decode(400_001, None).unwrap_err().contains("5-digit"));
    }
}

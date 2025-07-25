use crate::byteutil::Endian;

#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Back,
    SetEndian(Endian), // big or little
    Jump(usize),       // address to jump to
    Find(FindValue),   // value to find
    GTop,              // value to find
    GBottom,           // value to find
    Unknown(String),   // unknown command
}

#[derive(Debug, PartialEq)]
pub(crate) enum FindValue {
    Hex(Vec<u8>),
    Ascii(String),
}

impl Command {
    pub(crate) fn parse(input: &str) -> Command {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["b"] => Command::Back,
            ["g"] => Command::GTop,
            ["G"] => Command::GBottom,
            ["set", value] => {
                let value_parts: Vec<&str> = value.split('=').collect();
                match value_parts.as_slice() {
                    ["endian", endian] => {
                        let endian = match endian.to_lowercase().as_str() {
                            "big" => Endian::Big,
                            "little" => Endian::Little,
                            _ => return Command::Unknown(input.to_string()),
                        };
                        Command::SetEndian(endian)
                    }
                    _ => Command::Unknown(input.to_string()),
                }
            }
            ["j", address] if address.parse::<usize>().is_ok() => {
                Command::Jump(address.parse().unwrap())
            }
            ["f", value] => {
                if value.starts_with("0x") {
                    let hex_value = value.trim_start_matches("0x");
                    let bytes = hex::decode(hex_value).unwrap_or_else(|_| vec![]);
                    Command::Find(FindValue::Hex(bytes))
                } else {
                    Command::Find(FindValue::Ascii(value.to_string()))
                }
            }
            _ => Command::Unknown(input.to_string()),
        }
    }
}

mod test {
    use super::*;

    #[test]
    fn test_parse_command() {
        assert_eq!(
            Command::parse("set endian=big"),
            Command::SetEndian(Endian::Big)
        );
        assert_eq!(Command::parse("j 100"), Command::Jump(100));
        assert_eq!(
            Command::parse("f 0x4a0f99"),
            Command::Find(FindValue::Hex(vec![0x4a, 0x0f, 0x99]))
        );
        assert_eq!(
            Command::parse("f eeee"),
            Command::Find(FindValue::Ascii("eeee".to_string()))
        );
        assert!(matches!(
            Command::parse("unknown command"),
            Command::Unknown(_)
        ));
    }
}

use crate::byteutil::Endian;

#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Back,
    SetEndian(Endian), // big or little
    Jump(usize),       // address to jump to
    Find(String),      // value to find
    Unknown(String),   // unknown command
}

impl Command {
    pub(crate) fn parse(input: &str) -> Command {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["b"] => Command::Back,
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
            ["f", value] => Command::Find(value.to_string()),
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
        assert_eq!(Command::parse("jump 100"), Command::Jump(100));
        assert_eq!(
            Command::parse("find 0xFF"),
            Command::Find("0xFF".to_string())
        );
        assert!(matches!(
            Command::parse("unknown command"),
            Command::Unknown(_)
        ));
    }
}

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
    Cut(CutFile),
    CutSel(CutSelFile),
    Call(String),
    ListFunc,
}

#[derive(Debug, PartialEq)]
pub(crate) struct CutFile {
    count: usize,
    filepath: String,
}

impl CutFile {
    pub(crate) fn get_count(&self) -> usize {
        self.count
    }
    pub(crate) fn get_filepath(&self) -> &str {
        &self.filepath
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct CutSelFile {
    start: usize,
    end: usize,
    filepath: String,
}

impl CutSelFile {
    pub(crate) fn get_start(&self) -> usize {
        self.start
    }
    pub(crate) fn get_end(&self) -> usize {
        self.end
    }
    pub(crate) fn get_filepath(&self) -> &str {
        &self.filepath
    }
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
            ["lf"] => Command::ListFunc,
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
            ["cut", count, filepath] if count.parse::<usize>().is_ok() => Command::Cut(CutFile {
                count: count.parse().unwrap(),
                filepath: filepath.to_string(),
            }),
            ["cut", start, end, filepath] => {
                if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                    Command::CutSel(CutSelFile {
                        start: start,
                        end: end,
                        filepath: filepath.to_string(),
                    })
                } else {
                    Command::Unknown(input.to_string())
                }
            }
            ["call", function] => Command::Call(function.to_string()),
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

        assert_eq!(
            Command::parse("cut 0 10 xxx"),
            Command::CutSel(CutSelFile {
                start: 0,
                end: 10,
                filepath: "xxx".to_string()
            })
        );
        assert_eq!(
            Command::parse("cut 11 xxx"),
            Command::Cut(CutFile {
                count: 11,
                filepath: "xxx".to_string()
            })
        );
    }
}

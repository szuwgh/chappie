use crate::byteutil::Endian;
#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Empty,
    Back,
    SetEndian(Endian), // big or little
    Jump(usize),       // address to jump to
    Find(Value),       // value to find
    Search(Value),     // value to find
    Fuzzy(Value),      // value to find
    GTop,              // value to find
    GBottom,           // value to find
    StrInput(String),  // unknown command
    Cut(CutFile),
    CutSel(CutSelFile),
    Call(String),
    ListFunc,
    HexInput(Vec<u8>), // direct hex input like "F0 or F0AF"
    Insert(usize),     // number of bytes to insert
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
pub(crate) enum Value {
    Hex(Vec<u8>),
    Ascii(String),
}

impl Command {
    /// Get bytes from HexInput command
    pub(crate) fn get_hex_bytes(&self) -> Option<&[u8]> {
        match self {
            Command::HexInput(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Parse hex string like "0F" or "AF" into a single byte
    /// Only matches exactly 2 hex characters (1 byte)
    fn parse_hex_input(input: &str) -> Option<Vec<u8>> {
        // Remove all whitespace
        let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();

        // Check if all characters are valid hex digits
        if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }

        // Decode hex string to bytes
        hex::decode(cleaned).ok()
    }

    pub(crate) fn parse(input: &str) -> Command {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["/b"] => Command::Back,
            ["/g"] => Command::GTop,
            ["/G"] => Command::GBottom,
            ["/lf"] => Command::ListFunc,
            ["/set", value] => {
                let value_parts: Vec<&str> = value.split('=').collect();
                match value_parts.as_slice() {
                    ["endian", endian] => {
                        let endian = match endian.to_lowercase().as_str() {
                            "big" => Endian::Big,
                            "little" => Endian::Little,
                            _ => return Command::StrInput(input.to_string()),
                        };
                        Command::SetEndian(endian)
                    }
                    _ => Command::StrInput(input.to_string()),
                }
            }
            ["/j", address] if address.parse::<usize>().is_ok() => {
                Command::Jump(address.parse().unwrap())
            }
            ["/f", value] => {
                if value.starts_with("0x") {
                    let hex_value = value.trim_start_matches("0x");
                    let bytes = hex::decode(hex_value).unwrap_or_else(|_| vec![]);
                    Command::Find(Value::Hex(bytes))
                } else {
                    Command::Find(Value::Ascii(value.to_string()))
                }
            }
            ["/ss", value] => {
                if value.starts_with("0x") {
                    let hex_value = value.trim_start_matches("0x");
                    let bytes = hex::decode(hex_value).unwrap_or_else(|_| vec![]);
                    Command::Search(Value::Hex(bytes))
                } else {
                    Command::Search(Value::Ascii(value.to_string()))
                }
            }
            ["/s", value] => {
                if value.starts_with("0x") {
                    let hex_value = value.trim_start_matches("0x");
                    let bytes = hex::decode(hex_value).unwrap_or_else(|_| vec![]);
                    Command::Fuzzy(Value::Hex(bytes))
                } else {
                    Command::Fuzzy(Value::Ascii(value.to_string()))
                }
            }
            ["/cut", count, filepath] if count.parse::<usize>().is_ok() => Command::Cut(CutFile {
                count: count.parse().unwrap(),
                filepath: filepath.to_string(),
            }),
            ["/cut", start, end, filepath] => {
                if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                    Command::CutSel(CutSelFile {
                        start: start,
                        end: end,
                        filepath: filepath.to_string(),
                    })
                } else {
                    Command::StrInput(input.to_string())
                }
            }
            ["/call", function] => Command::Call(function.to_string()),
            ["/i", value] => {
                if let Ok(count) = value.parse::<usize>() {
                    Command::Insert(count)
                } else {
                    Command::StrInput(input.to_string())
                }
            }
            _ => {
                // Try to parse as hex input before returning StrInput
                if let Some(bytes) = Self::parse_hex_input(input) {
                    Command::HexInput(bytes)
                } else {
                    Command::StrInput(input.to_string())
                }
            }
        }
    }
}

mod test {
    use super::*;

    #[test]
    fn test_parse_command() {
        assert_eq!(
            Command::parse("/set endian=big"),
            Command::SetEndian(Endian::Big)
        );
        assert_eq!(Command::parse("/j 100"), Command::Jump(100));
        assert_eq!(
            Command::parse("/f 0x4a0f99"),
            Command::Find(Value::Hex(vec![0x4a, 0x0f, 0x99]))
        );
        assert_eq!(
            Command::parse("/f eeee"),
            Command::Find(Value::Ascii("eeee".to_string()))
        );
        assert!(matches!(
            Command::parse("unknown command"),
            Command::StrInput(_)
        ));

        assert_eq!(
            Command::parse("/cut 0 10 xxx"),
            Command::CutSel(CutSelFile {
                start: 0,
                end: 10,
                filepath: "xxx".to_string()
            })
        );
        assert_eq!(
            Command::parse("/cut 11 xxx"),
            Command::Cut(CutFile {
                count: 11,
                filepath: "xxx".to_string()
            })
        );
    }

    #[test]
    fn test_hex_input_single_byte() {
        // Test valid single byte hex inputs
        assert_eq!(Command::parse("0F"), Command::HexInput(vec![0x0F]));
        assert_eq!(Command::parse("AF"), Command::HexInput(vec![0xAF]));
        assert_eq!(Command::parse("FF"), Command::HexInput(vec![0xFF]));
        assert_eq!(Command::parse("00"), Command::HexInput(vec![0x00]));
        assert_eq!(Command::parse("A5"), Command::HexInput(vec![0xA5]));

        // Test lowercase
        assert_eq!(Command::parse("ab"), Command::HexInput(vec![0xAB]));
        assert_eq!(Command::parse("cd"), Command::HexInput(vec![0xCD]));

        // Test mixed case
        assert_eq!(Command::parse("Bf"), Command::HexInput(vec![0xBF]));

        // Test multiple bytes (now supported)
        assert_eq!(Command::parse("F0AF"), Command::HexInput(vec![0xF0, 0xAF]));
        assert_eq!(
            Command::parse("010203"),
            Command::HexInput(vec![0x01, 0x02, 0x03])
        );

        // Test invalid: odd length (not complete byte)
        assert!(matches!(Command::parse("F"), Command::StrInput(_)));
        assert!(matches!(Command::parse("F0A"), Command::StrInput(_)));

        // Test invalid: non-hex characters
        assert!(matches!(Command::parse("GG"), Command::StrInput(_)));
        assert!(matches!(Command::parse("XY"), Command::StrInput(_)));

        // Test get_hex_bytes method
        let cmd = Command::parse("AF");
        assert_eq!(cmd.get_hex_bytes(), Some(&[0xAF][..]));

        let cmd2 = Command::parse("/b");
        assert_eq!(cmd2.get_hex_bytes(), None);
    }
}

use crate::byteutil::Endian;
#[derive(Debug, PartialEq)]
pub(crate) enum HexCommand {
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
pub(crate) enum FindValue {
    Hex(Vec<u8>),
    Ascii(String),
}

impl HexCommand {
    /// Get bytes from HexInput command
    pub(crate) fn get_hex_bytes(&self) -> Option<&[u8]> {
        match self {
            HexCommand::HexInput(bytes) => Some(bytes),
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

    pub(crate) fn parse(input: &str) -> HexCommand {
        let parts: Vec<&str> = input.split_whitespace().collect();
        match parts.as_slice() {
            ["/b"] => HexCommand::Back,
            ["/g"] => HexCommand::GTop,
            ["/G"] => HexCommand::GBottom,
            ["/lf"] => HexCommand::ListFunc,
            ["/set", value] => {
                let value_parts: Vec<&str> = value.split('=').collect();
                match value_parts.as_slice() {
                    ["endian", endian] => {
                        let endian = match endian.to_lowercase().as_str() {
                            "big" => Endian::Big,
                            "little" => Endian::Little,
                            _ => return HexCommand::Unknown(input.to_string()),
                        };
                        HexCommand::SetEndian(endian)
                    }
                    _ => HexCommand::Unknown(input.to_string()),
                }
            }
            ["/j", address] if address.parse::<usize>().is_ok() => {
                HexCommand::Jump(address.parse().unwrap())
            }
            ["/f", value] => {
                if value.starts_with("0x") {
                    let hex_value = value.trim_start_matches("0x");
                    let bytes = hex::decode(hex_value).unwrap_or_else(|_| vec![]);
                    HexCommand::Find(FindValue::Hex(bytes))
                } else {
                    HexCommand::Find(FindValue::Ascii(value.to_string()))
                }
            }
            ["/cut", count, filepath] if count.parse::<usize>().is_ok() => {
                HexCommand::Cut(CutFile {
                    count: count.parse().unwrap(),
                    filepath: filepath.to_string(),
                })
            }
            ["/cut", start, end, filepath] => {
                if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                    HexCommand::CutSel(CutSelFile {
                        start: start,
                        end: end,
                        filepath: filepath.to_string(),
                    })
                } else {
                    HexCommand::Unknown(input.to_string())
                }
            }
            ["/call", function] => HexCommand::Call(function.to_string()),
            ["/i", value] => {
                if let Ok(count) = value.parse::<usize>() {
                    HexCommand::Insert(count)
                } else {
                    HexCommand::Unknown(input.to_string())
                }
            }
            _ => {
                // Try to parse as hex input before returning Unknown
                if let Some(bytes) = Self::parse_hex_input(input) {
                    HexCommand::HexInput(bytes)
                } else {
                    HexCommand::Unknown(input.to_string())
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
            HexCommand::parse("/set endian=big"),
            HexCommand::SetEndian(Endian::Big)
        );
        assert_eq!(HexCommand::parse("/j 100"), HexCommand::Jump(100));
        assert_eq!(
            HexCommand::parse("/f 0x4a0f99"),
            HexCommand::Find(FindValue::Hex(vec![0x4a, 0x0f, 0x99]))
        );
        assert_eq!(
            HexCommand::parse("/f eeee"),
            HexCommand::Find(FindValue::Ascii("eeee".to_string()))
        );
        assert!(matches!(
            HexCommand::parse("unknown command"),
            HexCommand::Unknown(_)
        ));

        assert_eq!(
            HexCommand::parse("/cut 0 10 xxx"),
            HexCommand::CutSel(CutSelFile {
                start: 0,
                end: 10,
                filepath: "xxx".to_string()
            })
        );
        assert_eq!(
            HexCommand::parse("/cut 11 xxx"),
            HexCommand::Cut(CutFile {
                count: 11,
                filepath: "xxx".to_string()
            })
        );
    }

    #[test]
    fn test_hex_input_single_byte() {
        // Test valid single byte hex inputs
        assert_eq!(HexCommand::parse("0F"), HexCommand::HexInput(vec![0x0F]));
        assert_eq!(HexCommand::parse("AF"), HexCommand::HexInput(vec![0xAF]));
        assert_eq!(HexCommand::parse("FF"), HexCommand::HexInput(vec![0xFF]));
        assert_eq!(HexCommand::parse("00"), HexCommand::HexInput(vec![0x00]));
        assert_eq!(HexCommand::parse("A5"), HexCommand::HexInput(vec![0xA5]));

        // Test lowercase
        assert_eq!(HexCommand::parse("ab"), HexCommand::HexInput(vec![0xAB]));
        assert_eq!(HexCommand::parse("cd"), HexCommand::HexInput(vec![0xCD]));

        // Test mixed case
        assert_eq!(HexCommand::parse("Bf"), HexCommand::HexInput(vec![0xBF]));

        // Test multiple bytes (now supported)
        assert_eq!(
            HexCommand::parse("F0AF"),
            HexCommand::HexInput(vec![0xF0, 0xAF])
        );
        assert_eq!(
            HexCommand::parse("010203"),
            HexCommand::HexInput(vec![0x01, 0x02, 0x03])
        );

        // Test invalid: odd length (not complete byte)
        assert!(matches!(HexCommand::parse("F"), HexCommand::Unknown(_)));
        assert!(matches!(HexCommand::parse("F0A"), HexCommand::Unknown(_)));

        // Test invalid: non-hex characters
        assert!(matches!(HexCommand::parse("GG"), HexCommand::Unknown(_)));
        assert!(matches!(HexCommand::parse("XY"), HexCommand::Unknown(_)));

        // Test get_hex_bytes method
        let cmd = HexCommand::parse("AF");
        assert_eq!(cmd.get_hex_bytes(), Some(&[0xAF][..]));

        let cmd2 = HexCommand::parse("/b");
        assert_eq!(cmd2.get_hex_bytes(), None);
    }
}

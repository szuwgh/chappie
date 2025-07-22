use half::f16;

pub(crate) struct ByteView(Vec<u8>);

impl ByteView {
    pub(crate) fn new(data: Vec<u8>) -> Self {
        ByteView(data)
    }

    pub(crate) fn to_binary_8bit(&self) -> String {
        //只转化第一个u8
        self.0
            .get(0)
            .map_or("00000000".to_string(), |&b| format!("{:08b}", b))
    }

    pub(crate) fn to_u8(&self) -> u8 {
        *self.0.get(0).unwrap_or(&0)
    }

    pub(crate) fn to_u16(&self) -> u16 {
        u16::from_le_bytes([*self.0.get(0).unwrap_or(&0), *self.0.get(1).unwrap_or(&0)])
    }

    pub(crate) fn to_i16(&self) -> i16 {
        i16::from_le_bytes([*self.0.get(0).unwrap_or(&0), *self.0.get(1).unwrap_or(&0)])
    }

    pub(crate) fn to_u24(&self) -> u32 {
        let bytes = [
            *self.0.get(0).unwrap_or(&0),
            *self.0.get(1).unwrap_or(&0),
            *self.0.get(2).unwrap_or(&0),
        ];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
    }

    pub(crate) fn to_i24(&self) -> i32 {
        let bytes = [
            *self.0.get(0).unwrap_or(&0),
            *self.0.get(1).unwrap_or(&0),
            *self.0.get(2).unwrap_or(&0),
        ];
        i32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
    }

    pub(crate) fn to_u32(&self) -> u32 {
        u32::from_le_bytes([
            *self.0.get(0).unwrap_or(&0),
            *self.0.get(1).unwrap_or(&0),
            *self.0.get(2).unwrap_or(&0),
            *self.0.get(3).unwrap_or(&0),
        ])
    }

    pub(crate) fn to_f16(&self) -> f16 {
        let bytes = [*self.0.get(0).unwrap_or(&0), *self.0.get(1).unwrap_or(&0)];
        f16::from_le_bytes(bytes)
    }
}

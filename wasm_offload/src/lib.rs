pub use wasm_offload_procmacro::offload;

#[derive(Clone, Debug)]
pub enum Val {
    Bool(bool),
    S8(i8),
    U8(u8),
    S16(i16),
    U16(u16),
    S32(i32),
    U32(u32),
    S64(i64),
    U64(u64),
    Float32(f32),
    Float64(f64),
    Char(char),
    String(String),
    List(Vec<Val>),
    Record(Vec<(String, Val)>),
    Tuple(Vec<Val>),
    Variant(String, Option<Box<Val>>),
    Enum(String),
    Option(Option<Box<Val>>),
    Result(Result<Option<Box<Val>>, Option<Box<Val>>>),
    Flags(Vec<String>),
}

impl Val {
    pub fn into_i32(self) -> i32 {
        match self {
            Val::S32(v) => v,
            _ => panic!(),
        }
    }
}

impl From<bool> for Val {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i8> for Val {
    fn from(value: i8) -> Self {
        Self::S8(value)
    }
}

impl From<i16> for Val {
    fn from(value: i16) -> Self {
        Self::S16(value)
    }
}

impl From<i32> for Val {
    fn from(value: i32) -> Self {
        Self::S32(value)
    }
}

impl From<i64> for Val {
    fn from(value: i64) -> Self {
        Self::S64(value)
    }
}

impl From<u8> for Val {
    fn from(value: u8) -> Self {
        Self::U8(value)
    }
}

impl From<u16> for Val {
    fn from(value: u16) -> Self {
        Self::U16(value)
    }
}

impl From<u32> for Val {
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}

impl From<u64> for Val {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

pub trait OffloadTarget {
    type Error;

    fn initialize(&mut self) -> Result<(), Self::Error>;

    fn call_function(
        &mut self,
        module: &[u8],
        name: &str,
        args: &[Val],
        returns: bool,
    ) -> Result<Option<Val>, Self::Error>;
}

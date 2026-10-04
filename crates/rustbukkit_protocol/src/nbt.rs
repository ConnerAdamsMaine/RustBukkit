pub enum Tag<'a> {
    Byte(i8),
    Int(i32),
    LongArray(Vec<i64>),
    Float(f32),
    Double(f64),
    String(&'a str),
    Compound(Vec<(&'a str, Tag<'a>)>),
    CompoundList(Vec<Tag<'a>>),
    StringList(Vec<&'a str>),
}

impl Tag<'_> {
    fn kind(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Int(_) => 3,
            Self::LongArray(_) => 12,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::String(_) => 8,
            Self::Compound(_) => 10,
            Self::CompoundList(_) | Self::StringList(_) => 9,
        }
    }

    fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Self::Byte(value) => out.push(*value as u8),
            Self::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::LongArray(values) => {
                out.extend_from_slice(&(values.len() as i32).to_be_bytes());
                for value in values {
                    out.extend_from_slice(&value.to_be_bytes());
                }
            }
            Self::Float(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::Double(value) => out.extend_from_slice(&value.to_be_bytes()),
            Self::String(value) => write_string(out, value),
            Self::Compound(fields) => {
                for (name, value) in fields {
                    out.push(value.kind());
                    write_string(out, name);
                    value.write_payload(out);
                }
                out.push(0);
            }
            Self::CompoundList(values) => {
                out.push(10);
                out.extend_from_slice(&(values.len() as i32).to_be_bytes());
                for value in values {
                    value.write_payload(out);
                }
            }
            Self::StringList(values) => {
                out.push(8);
                out.extend_from_slice(&(values.len() as i32).to_be_bytes());
                for value in values {
                    write_string(out, value);
                }
            }
        }
    }
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

pub fn encode_root(fields: Vec<(&str, Tag<'_>)>) -> Vec<u8> {
    let mut out = vec![10, 0, 0];
    Tag::Compound(fields).write_payload(&mut out);
    out
}

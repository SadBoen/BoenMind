/// 为「线上表示是固定字符串」的合同枚举生成 as_str / from_wire / Display / Serde。
/// 字符串形态以合同 schema 的 enum 值为准,同步测试保证一致。
macro_rules! wire_str_enum {
    ($name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $( $variant ),+ }
        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self { $( $name::$variant => $wire ),+ }
            }
            pub fn from_wire(s: &str) -> Option<Self> {
                match s { $( $wire => Some($name::$variant), )+ _ => None }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct V;
                impl serde::de::Visitor<'_> for V {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, concat!(stringify!($name), " 线上字符串"))
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                        $name::from_wire(v)
                            .ok_or_else(|| E::custom(format_args!("非法 {} 字面量: {:?}", stringify!($name), v)))
                    }
                }
                deserializer.deserialize_str(V)
            }
        }
    };
}

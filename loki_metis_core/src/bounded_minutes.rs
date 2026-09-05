//! 供不同业务域的分钟数值域 newtype 共用的边界校验、时长换算与错误定义。

/// 生成一个校验范围为 `[min, max]` 分钟的独立 newtype，含 `new`/`get`/`duration`、
/// `Default` 与配套错误类型。每次调用生成的类型互不兼容，即使范围相同也无法混用，
/// 避免调用方把不同业务域（如扫描间隔与 CollectProvider 上报间隔）的分钟数意外传混。
macro_rules! bounded_minutes_newtype {
    (
        $(#[$type_doc:meta])+
        struct $name:ident;
        min = $min:expr;
        max = $max:expr;
        default = $default:expr;
        $(#[$error_doc:meta])+
        error $error:ident = $message:literal;
    ) => {
        $(#[$type_doc])+
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(u16);

        impl $name {
            /// 校验分钟数位于批准范围，返回已确认合法的值。
            pub const fn new(minutes: u16) -> Result<Self, $error> {
                if minutes < $min || minutes > $max {
                    return Err($error);
                }
                Ok(Self(minutes))
            }

            /// 返回用于设置持久化和 GUI 展示的整数分钟。
            pub const fn get(self) -> u16 {
                self.0
            }

            /// 返回标准库时长；分钟先提升为 u64 再乘 60，避免 u16 上限溢出。
            pub const fn duration(self) -> ::std::time::Duration {
                ::std::time::Duration::from_secs(self.0 as u64 * 60)
            }
        }

        impl Default for $name {
            /// 返回该业务域批准的默认分钟数。
            fn default() -> Self {
                Self($default)
            }
        }

        $(#[$error_doc])+
        #[derive(Debug, Clone, Copy, PartialEq, Eq, ::thiserror::Error)]
        #[error($message)]
        pub struct $error;
    };
}

pub(crate) use bounded_minutes_newtype;

use clap::Parser;

#[derive(Debug, Parser)]
#[clap(version, about)]
pub struct Args {
    /// 配置文件路径
    #[arg(short, long, default_value = "/etc/rdhcpd.yml")]
    pub cfg: String,
}

impl Args {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

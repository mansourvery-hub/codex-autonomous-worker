use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "autopilot")]
#[command(author = "Autopilot Contributors")]
#[command(version = "0.4.0")]
#[command(about = "24/7 Autonomous Engineering Daemon and Terminal Control Deck", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch the split-screen TUI control deck
    Tui,

    /// Launch a 24/7 autonomous continuous loop and open the control deck
    Loop {
        /// Campaign objective
        #[arg(default_value = "Audit domain logic, write reproduction tests first, fix edge cases, and execute PLAN.md iteratively")]
        prompt: Vec<String>,

        /// Target repository name
        #[arg(short, long, default_value = "chess-repertoire-srs")]
        repo: String,

        /// Model override
        #[arg(short, long)]
        model: Option<String>,

        /// Iterations budget
        #[arg(long, default_value_t = 30)]
        iterations: u32,
    },

    /// Queue a new autonomous engineering campaign
    Queue {
        /// Campaign objective
        prompt: Vec<String>,

        /// Target repository name
        #[arg(short, long)]
        repo: Option<String>,

        /// Model override
        #[arg(short, long)]
        model: Option<String>,

        /// Iteration budget for continuous loop
        #[arg(long, default_value_t = 20)]
        iterations: u32,
    },

    /// List active, queued, and completed campaigns
    List,

    /// Show currently running campaign details
    Current,

    /// Show completed campaigns history
    History {
        #[arg(short = 'n', long, default_value_t = 10)]
        count: usize,
    },

    /// View or follow logs
    Logs {
        #[arg(short, long)]
        follow: bool,

        #[arg(short, long, default_value_t = 30)]
        lines: usize,

        #[arg(long)]
        last: bool,
    },

    /// Run the 24/7 background supervisor daemon (used by systemd)
    Daemon,
}

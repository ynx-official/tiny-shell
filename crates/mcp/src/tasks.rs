use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    AwaitingUser,
    Running,
    Succeeded,
    Failed,
    CancelRequested,
    Cancelled,
    TimedOut,
    Interrupted,
    OutcomeUnknown,
}
impl State {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Cancelled
                | Self::TimedOut
                | Self::Interrupted
                | Self::OutcomeUnknown
        )
    }
    pub fn permits(self, next: Self) -> bool {
        match self {
            Self::Queued => matches!(
                next,
                Self::AwaitingUser
                    | Self::Running
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Interrupted
            ),
            Self::AwaitingUser => matches!(
                next,
                Self::Queued | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Running => matches!(
                next,
                Self::Succeeded
                    | Self::Failed
                    | Self::CancelRequested
                    | Self::TimedOut
                    | Self::OutcomeUnknown
                    | Self::AwaitingUser
                    | Self::Interrupted
            ),
            Self::CancelRequested => matches!(
                next,
                Self::OutcomeUnknown | Self::Succeeded | Self::Failed | Self::Interrupted
            ),
            _ => false,
        }
    }
}

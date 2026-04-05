use crate::tauri_api::{HotkeyActivityState, HotkeyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveRecordingCommand {
    Start,
    Stop,
}

pub(super) fn desired_recording_goal(
    mode: HotkeyMode,
    state: HotkeyActivityState,
    current_recording: bool,
    desired_recording: bool,
) -> Option<bool> {
    match mode {
        HotkeyMode::PushToTalk => Some(matches!(state, HotkeyActivityState::Pressed)),
        HotkeyMode::Toggle => match state {
            HotkeyActivityState::Pressed => Some(if current_recording == desired_recording {
                !current_recording
            } else {
                !desired_recording
            }),
            HotkeyActivityState::Released => None,
        },
    }
}

pub(super) fn next_command_for_goal(
    is_recording: bool,
    desired_recording: bool,
) -> Option<LiveRecordingCommand> {
    match (is_recording, desired_recording) {
        (false, true) => Some(LiveRecordingCommand::Start),
        (true, false) => Some(LiveRecordingCommand::Stop),
        _ => None,
    }
}

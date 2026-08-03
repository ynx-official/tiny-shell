use std::collections::VecDeque;

use crate::app::settings::MonitoringPosition;

/// 监控领域的纯数据变换。
///
/// UI 只负责读取 `TinyShell` 状态和渲染；网络历史平滑、坐标尺度和单位格式化
/// 不依赖 GPUI，因此可以独立测试，也不会把边界条件扩散到页面代码中。
pub(crate) struct MonitoringVisibilityContext {
    pub(crate) position: MonitoringPosition,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) system_info_open: bool,
    pub(crate) active_tab_open: bool,
    pub(crate) active_tab_is_ssh: bool,
    pub(crate) home_page_open: bool,
}

pub(crate) fn metrics_visible(context: MonitoringVisibilityContext) -> bool {
    context.system_info_open
        || (!context.sidebar_collapsed
            && (context.active_tab_is_ssh || context.position == MonitoringPosition::Sidebar))
        || (context.active_tab_open
            && !context.home_page_open
            && context.position == MonitoringPosition::Bottom)
}

pub(crate) fn push_bounded(history: &mut VecDeque<f32>, value: f32, capacity: usize) {
    if capacity == 0 {
        history.clear();
        return;
    }
    while history.len() >= capacity {
        history.pop_front();
    }
    history.push_back(value);
}

pub(super) fn smooth_monitoring_series(values: &[f32]) -> Vec<f32> {
    let Some((&first, rest)) = values.split_first() else {
        return Vec::new();
    };
    let mut smoothed = Vec::with_capacity(values.len());
    smoothed.push(first.max(0.0));
    let mut previous = first.max(0.0);
    for &value in rest {
        previous = previous * 0.58 + value.max(0.0) * 0.42;
        smoothed.push(previous);
    }
    smoothed
}

pub(super) fn nice_network_scale(max_value: f32) -> f32 {
    if !max_value.is_finite() || max_value <= 1.0 {
        return 1.0;
    }
    let magnitude = 10_f32.powf(max_value.log10().floor());
    let normalized = max_value / magnitude;
    let step = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * magnitude
}

pub(super) fn format_network_axis(bytes_per_second: f32) -> String {
    let bytes_per_second = bytes_per_second.max(0.0);
    let (value, unit) = if bytes_per_second >= 1024.0 * 1024.0 * 1024.0 {
        (bytes_per_second / (1024.0 * 1024.0 * 1024.0), "G")
    } else if bytes_per_second >= 1024.0 * 1024.0 {
        (bytes_per_second / (1024.0 * 1024.0), "M")
    } else if bytes_per_second >= 1024.0 {
        (bytes_per_second / 1024.0, "K")
    } else {
        (bytes_per_second, "B")
    };
    if value >= 10.0 {
        format!("{value:.0}{unit}")
    } else {
        format!("{value:.1}{unit}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::app::settings::MonitoringPosition;

    use super::{
        MonitoringVisibilityContext, format_network_axis, metrics_visible, nice_network_scale,
        push_bounded, smooth_monitoring_series,
    };

    #[test]
    fn bottom_monitoring_samples_local_terminal_when_panel_is_visible() {
        assert!(metrics_visible(MonitoringVisibilityContext {
            position: MonitoringPosition::Bottom,
            sidebar_collapsed: true,
            system_info_open: false,
            active_tab_open: true,
            active_tab_is_ssh: false,
            home_page_open: false,
        }));
        assert!(!metrics_visible(MonitoringVisibilityContext {
            position: MonitoringPosition::Bottom,
            sidebar_collapsed: false,
            system_info_open: false,
            active_tab_open: true,
            active_tab_is_ssh: false,
            home_page_open: true,
        }));
    }

    #[test]
    fn hidden_monitoring_never_samples_local_terminal() {
        assert!(!metrics_visible(MonitoringVisibilityContext {
            position: MonitoringPosition::Hidden,
            sidebar_collapsed: false,
            system_info_open: false,
            active_tab_open: true,
            active_tab_is_ssh: false,
            home_page_open: false,
        }));
    }

    #[test]
    fn bounded_history_discards_oldest_values_and_accepts_zero_capacity() {
        let mut history = VecDeque::from([1.0, 2.0]);
        push_bounded(&mut history, 3.0, 2);
        assert_eq!(history, VecDeque::from([2.0, 3.0]));

        push_bounded(&mut history, 4.0, 0);
        assert!(history.is_empty());
    }

    #[test]
    fn smoothing_preserves_length_and_clamps_negative_samples() {
        let values = smooth_monitoring_series(&[1.0, -1.0, 3.0]);
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], 1.0);
        assert!(values.iter().all(|value| *value >= 0.0));
    }

    #[test]
    fn network_scale_handles_non_finite_and_boundary_values() {
        assert_eq!(nice_network_scale(f32::NAN), 1.0);
        assert_eq!(nice_network_scale(f32::INFINITY), 1.0);
        assert_eq!(nice_network_scale(0.0), 1.0);
        assert_eq!(nice_network_scale(1.1), 2.0);
        assert_eq!(nice_network_scale(5.1), 10.0);
    }

    #[test]
    fn network_axis_uses_binary_units_and_non_negative_output() {
        assert_eq!(format_network_axis(0.0), "0.0B");
        assert_eq!(format_network_axis(-1.0), "0.0B");
        assert_eq!(format_network_axis(1024.0), "1.0K");
        assert_eq!(format_network_axis(10.0 * 1024.0), "10K");
    }
}

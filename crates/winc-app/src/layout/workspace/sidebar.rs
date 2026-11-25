//! 侧栏顶上的几何：文件树工具栏、折叠工具、视图切换的分段按钮、红绿灯让出的位置。

use super::*;
use winc_i18n::t;

/// 文件树工具栏上的按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeTool {
    /// 新建文件。
    NewFile,
    /// 新建目录。
    NewFolder,
    /// 刷新目录。
    Refresh,
    /// 折叠所有目录。
    CollapseAll,
    /// 在文件树中定位当前文件。
    Locate,
}

impl TreeTool {
    /// 全部工具，顺序与工具栏从左到右的顺序一致。
    pub const ALL: [TreeTool; 5] = [
        TreeTool::NewFile,
        TreeTool::NewFolder,
        TreeTool::Refresh,
        TreeTool::CollapseAll,
        TreeTool::Locate,
    ];

    /// 返回悬停提示文字。
    pub fn label(self) -> &'static str {
        match self {
            TreeTool::NewFile => t!("workspace-tree-tool-new-file"),
            TreeTool::NewFolder => t!("workspace-tree-tool-new-folder"),
            TreeTool::Refresh => t!("workspace-tree-tool-refresh"),
            TreeTool::CollapseAll => t!("workspace-tree-tool-collapse-all"),
            TreeTool::Locate => t!("workspace-tree-tool-locate"),
        }
    }
}

/// 项目名（含箭头）所占宽度的下限（逻辑像素）：侧栏最窄时仍要露出名字的开头和折叠箭头，
/// 工具栏不占用这部分宽度。
pub const TREE_NAME_MIN: f32 = 48.0;

/// 返回文件树工具栏的按钮布局：位于项目名所在行的右端，从左到右依次是新建文件、新建目录、刷新、
/// 全部折叠、定位。
///
/// 放不下时从右向左收起：留下的按钮仍然显示，收起的按钮移入「⋯」菜单。返回留下的按钮（从左到右）
/// 和「⋯」按钮的位置，全部放得下时后者为 `None`。`name_right` 是项目名和箭头的右边界，
/// 工具栏不会越过该边界。
pub fn tree_tools(header: Rect, name_right: f32) -> (Vec<(Rect, TreeTool)>, Option<Rect>) {
    let right = header.right() - 8.0;
    // 可容纳的按钮数；有按钮被收起时，其中一格留给「⋯」，刚好全部放下时不显示「⋯」。
    let room = ((right - name_right + BUTTON_GAP) / (BUTTON_SMALL + BUTTON_GAP))
        .floor()
        .max(0.0) as usize;
    let all = TreeTool::ALL.len();
    let button = |i, count| toolbar_button(header, right, i, count);

    if room >= all {
        let buttons = (0..all)
            .map(|i| (button(i, all), TreeTool::ALL[i]))
            .collect();
        return (buttons, None);
    }

    if room == 0 {
        return (Vec::new(), None);
    }

    // 最右侧一格留给「⋯」，其余各格按顺序留给最靠前的按钮。
    let kept = room - 1;
    let buttons = (0..kept)
        .map(|i| (button(i, room), TreeTool::ALL[i]))
        .collect();
    (buttons, Some(button(kept, room)))
}

/// 返回收起动画途中文件树工具栏的绘制位置：`shown` 是各工具（按 [`TreeTool::ALL`] 的顺序）
/// 显示出来的程度，`more` 是「⋯」按钮的，0 表示收起，1 表示完全显示。
///
/// 从右往左排：「⋯」固定在最右一格，每个工具按自身程度占用对应格数。正在收起的工具向右滑到右侧
/// 相邻的按钮上（最终叠进「⋯」），左边的按钮跟着右移补上空位；程度都是 0 或 1 时与 [`TreeState::tools`]
/// 的结果相同。返回显示程度大于 0 的工具（从左到右）及其程度，以及「⋯」按钮。
pub fn folding_tools(
    header: Rect,
    shown: &[f32; TreeTool::ALL.len()],
    more: f32,
) -> (Vec<(Rect, TreeTool, f32)>, Option<Rect>) {
    let right = header.right() - 8.0;
    let step = BUTTON_SMALL + BUTTON_GAP;
    let y = header.y + (header.h - BUTTON_SMALL) / 2.0;

    let mut cursor = right - more * step;
    let mut tools = Vec::new();
    for (i, &tool) in TreeTool::ALL.iter().enumerate().rev() {
        if shown[i] <= 0.0 {
            continue;
        }
        cursor -= shown[i] * step;
        let rect = Rect::new(cursor + BUTTON_GAP, y, BUTTON_SMALL, BUTTON_SMALL);
        tools.push((rect, tool, shown[i]));
    }
    tools.reverse();

    let more = (more > 0.0).then(|| toolbar_button(header, right, 0, 1));
    (tools, more)
}

/// 返回右对齐于 `right` 的一排 `count` 个工具栏按钮中第 `i` 个（从 0 起）的位置，在 `bar`
/// 内垂直居中。
pub(in crate::layout) fn toolbar_button(bar: Rect, right: f32, i: usize, count: usize) -> Rect {
    let step = BUTTON_SMALL + BUTTON_GAP;
    let x = right - (count - i) as f32 * step + BUTTON_GAP;
    let y = bar.y + (bar.h - BUTTON_SMALL) / 2.0;
    Rect::new(x, y, BUTTON_SMALL, BUTTON_SMALL)
}

/// 返回工具栏收起后移入「⋯」菜单的按钮；`kept` 是工具栏留下的按钮数。
pub fn folded_tools(kept: usize) -> &'static [TreeTool] {
    &TreeTool::ALL[kept.min(TreeTool::ALL.len())..]
}

/// 侧栏显示的内容：文件树、全局搜索，或 Git 的提交视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarView {
    /// 文件树。
    #[default]
    Files,
    /// 全局搜索面板。
    Search,
    /// Git 的提交视图：改动列表、提交信息。
    ///
    /// 只在 Git 模块开着、目录在仓库里时有。
    Commit,
}

impl SidebarView {
    /// 返回悬停提示文字（含快捷键）。
    pub fn label(self) -> String {
        match self {
            SidebarView::Files => with_key(t!("workspace-sidebar-files"), Some(Action::ShowFiles)),
            SidebarView::Search => {
                with_key(t!("workspace-sidebar-search"), Some(Action::FindInFiles))
            }
            SidebarView::Commit => {
                with_key(t!("workspace-sidebar-commit"), Some(Action::ShowCommit))
            }
        }
    }
}

/// 红绿灯右端距窗口左边缘的距离（逻辑像素），以 `f32` 的位存放；见 [`TRAFFIC_LIGHTS_END`]。
static LIGHTS_END: AtomicU32 = AtomicU32::new(TRAFFIC_LIGHTS_END.to_bits());

/// 返回红绿灯右端距窗口左边缘的距离（逻辑像素）；没有红绿灯时为 0。
pub fn traffic_lights_end() -> f32 {
    f32::from_bits(LIGHTS_END.load(Ordering::Relaxed))
}

/// 记录红绿灯右端的实际位置（窗口创建、摆放红绿灯后调用），返回位置是否有变化。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn set_traffic_lights_end(end: f32) -> bool {
    LIGHTS_END.swap(end.to_bits(), Ordering::Relaxed) != end.to_bits()
}

/// 总是有的两个侧栏视图。
pub const BASE_VIEWS: [SidebarView; 2] = [SidebarView::Files, SidebarView::Search];

/// 分段控件里相邻两段之间的缝：选中一段凸起时与旁边一段分开，不贴在一起。
pub const SEGMENT_SPACING: f32 = 2.0;

/// 活动栏中每个视图图标格的高度（逻辑像素），与 `ACTIVITY_BAR_W` 等宽构成 48×48 正方形按钮。
pub const ACTIVITY_ITEM_H: f32 = ACTIVITY_BAR_W;

/// 返回最左侧垂直活动栏里各视图图标（「文件」「搜索」「源代码管理」）的位置，顺序同 `views`：
/// 自 `activity_bar` 顶部依次向下排列，每格宽 `ACTIVITY_BAR_W`、高 `ACTIVITY_ITEM_H * share`。
pub fn view_tabs(activity_bar: Rect, views: &[(SidebarView, f32)]) -> Vec<(Rect, SidebarView)> {
    let mut y = activity_bar.y;
    views
        .iter()
        .map(|&(view, share)| {
            let h = (ACTIVITY_ITEM_H * share).round();
            let rect = Rect::new(activity_bar.x, y, activity_bar.w, h);
            y += h;
            (rect, view)
        })
        .collect()
}

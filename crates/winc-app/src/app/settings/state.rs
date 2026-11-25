//! 设置浮层的状态类型、格式化行模型与数值步进辅助函数。

use std::ops::RangeInclusive;
use std::time::Instant;

use winc_editor::format::Task;
use winc_i18n::t;

use crate::app::{FontError, FontSpec, Setting, SettingsPart, ThemeChoice, unusable_font};
use crate::field::TextField;
use crate::keymap;
use crate::layout::ToolLanguages;

use super::shortcuts;

/// 设置浮层的状态。
#[derive(Debug, Default)]
pub struct SettingsState {
    /// 浮层是否打开。
    ///
    /// 打开时覆盖整个窗口，键盘和鼠标都先交给浮层。
    pub open: bool,
    /// 当前选中的设置项，由键盘上下键或鼠标点击选中；右侧显示的是该设置项所在的那一页。
    pub selected: Setting,
    /// 是否用键盘移动过选中项。
    ///
    /// 只有此时才给选中的那一行铺底色，使用鼠标时不显示。
    pub keyboard: bool,
    /// 正在编辑字体名或格式化命令时，对应的设置项和输入框的当前内容。
    pub editing: Option<(Setting, TextField)>,
    /// 开始编辑前的字体设置；按 `Esc` 或关闭浮层时退回该值。
    pub editing_original: Option<String>,
    /// 字体名无法识别等错误：出错的设置项和原因，原因用错误色写在该行输入框的左侧。
    ///
    /// 离开输入框后，输入退回原值，原因仍留在该行，换页、再次编辑或关闭浮层时清除。
    pub error: Option<(Setting, String)>,
    /// 当前页内容的纵向滚动位置（逻辑像素），换页时回到顶部。
    pub scroll: f32,
    /// 鼠标悬停的部件。
    pub hover: Option<SettingsPart>,
    /// 格式化页列出的语言。
    ///
    /// 打开设置时确定，打开期间只会增加，不会减少，已列出的语言也不换位置。
    pub languages: ToolLanguages,
    /// 格式化页每一行（格式化工具、修复工具）上显示的内容。
    pub tool_rows: Vec<(Setting, FormatRow)>,
    /// 正在后台查询 WSL 发行版中装了哪些工具，查完后重新计算各行。
    pub probing: bool,
    /// 分类栏顶部的搜索框：按设置项的名称和说明搜索，不为空白时正文换成搜索结果。
    pub search: TextField,
    /// 「关于」页顶部的产品图标：渲染时的边长（物理像素）和 RGBA 像素；关闭浮层时释放。
    ///
    /// 渲染失败时像素为空，不再重试，图标不显示。
    pub logo: Option<(u32, Vec<u8>)>,
    /// 快捷键页的搜索框：按命令名或按键搜索。
    pub keys_query: TextField,
    /// 是否正在录制按键：按下的键写入搜索框，不执行。
    pub recording: bool,
    /// 搜索框中的内容是否为录制的按键（按键搜索，而不是按名称搜索）。
    pub keys_recorded: bool,
    /// 「快捷键」页中正在修改的一行。
    pub(crate) key_edit: Option<shortcuts::KeyEdit>,
    /// 「快捷键」页中一行展开、收起的过渡：同时收起的另一行和时钟；展开的是 `key_edit` 的一行。
    pub key_slide: Option<(Option<keymap::Action>, crate::app::animation::Clock)>,
    /// 「导入…」菜单列出的来源，打开菜单时查找。
    pub(crate) import_sources: Vec<crate::keymap::import::Source>,
    /// 浮层正在出现（`true`）或消失（`false`）的动画：出现时遮罩淡入，卡片整张淡入、从 95%
    /// 放大到原大；消失时整张淡出、略微缩小。
    ///
    /// 消失期间 `open` 已经是 `false`，键盘和鼠标已经交回主界面，此时仍需绘制最后一帧。
    pub motion: Option<(bool, crate::app::animation::Clock)>,
    /// 「模式」换了以后，凸起的那一段从原来那一段（`ThemeChoice`）滑过去。
    pub mode_slide: Option<(ThemeChoice, crate::app::animation::Clock)>,
    /// 刚切换的开关（`Setting`）：圆钮滑到另一侧，底色渐变到新状态的颜色。
    pub switch_slide: Option<(Setting, crate::app::animation::Clock)>,
    /// 出现、消失动画用的快照，动画开头画一次存下来，动画结束就释放。
    pub cache: Option<DialogCache>,
    /// 设置页滚动或拖动期间保存的窗口画面（含遮罩），用于代替逐帧重绘浮层下方的界面。
    pub scroll_base: Option<winc_render::Snapshot>,
    /// 最近一次滚动或拖动的时刻；两者都未进行时为 `None`。
    pub scrolled_at: Option<Instant>,
    /// 卡片相对窗口中央的偏移（逻辑像素），拖动标题行时改变；窗口存在期间保留，再次打开时
    /// 卡片仍在原处。
    pub offset: (f32, f32),
}

/// 设置浮层出现、消失的动画用的两张快照：动画开头画一次，途中每帧直接拿来用。
#[derive(Debug)]
pub struct DialogCache {
    /// 浮层底下的整个窗口（主界面，不含浮层）。
    pub base: winc_render::Snapshot,
    /// 画好的卡片：原大、不透明，不含阴影。
    pub card: winc_render::Snapshot,
}

/// 格式化页中一种语言的一行（格式化工具或修复工具）：右侧显示工具，名称下方显示来源和提醒。
#[derive(Debug, Clone, PartialEq)]
pub struct FormatRow {
    /// 按钮上显示的文字：工具名（例如「Ruff」）；自己写的命令为「自定义」；
    /// 没有可用工具时为「未设置」。
    pub tool: String,
    /// 是否没有可用的工具（按钮上写「未设置」）：此时按钮文字画成淡色。
    pub unset: bool,
    /// 名称下方那一行：工具的确定方式（例如「自动 · 根据 pyproject.toml」），
    /// 指定了命令时为命令本身。
    pub detail: String,
    /// 自动选择会选中的工具（例如「Ruff（pyproject.toml）」），显示在菜单中「自动」之下；
    /// 选不出来时为 `None`。
    pub auto: Option<String>,
    /// 项目设置（`.winc/config.json`）中写的命令。
    ///
    /// 存在时，该项目内使用此命令，不看用户设置。
    pub project: Option<String>,
    /// 「未安装」等提醒，接在 `detail` 之后。
    pub warning: Option<String>,
    /// 没装但知道安装方法的工具，菜单中有「安装方法…」。
    pub install: Vec<String>,
    /// 能否从列表中移除（菜单中有「移除」）：只有格式化工具一行，且这种语言是用户自己加的
    /// （或写过命令），不看用户设置时不会列出。
    pub removable: bool,
}

impl ToolLanguages {
    /// 返回 `task`（格式化或修复）那一组列出的语言。
    pub(super) fn of(&mut self, task: Task) -> &mut Vec<&'static str> {
        match task {
            Task::Format => &mut self.format,
            Task::Fix => &mut self.fix,
        }
    }
}

impl SettingsState {
    /// 返回设置项 `setting`（`FormatCommand` 或 `FixCommand`）那一行上显示的内容。
    pub fn tool_row(&self, setting: Setting) -> Option<&FormatRow> {
        self.tool_rows
            .iter()
            .find(|(s, _)| *s == setting)
            .map(|(_, r)| r)
    }
}

/// 返回工具设置项对应的（任务种类，语言 ID）；不是工具设置项时返回 `None`。
pub(super) fn tool_setting(setting: Setting) -> Option<(Task, &'static str)> {
    match setting {
        Setting::FormatCommand(id) => Some((Task::Format, id)),
        Setting::FixCommand(id) => Some((Task::Fix, id)),
        _ => None,
    }
}

/// 返回字体设置值 `value` 不能用的原因；未设置或能用时返回 `None`。
pub(super) fn font_error(value: &Option<String>) -> Option<String> {
    let (spec, name, error) = unusable_font(value)?;
    let name = name.as_str();
    Some(match (error, spec) {
        (FontError::NotFound, FontSpec::Name(_)) => t!("settings-font-not-installed", name = name),
        (FontError::NotFound, FontSpec::Path(_)) => t!("settings-font-file-not-found", name = name),
        (FontError::Unsupported, _) => t!("settings-font-unsupported", name = name),
    })
}

/// 界面缩放的档位（百分比）：设置菜单中列出，快捷键依次经过。
///
/// 100% 附近每档 10%，便于微调；更大的倍数每档 25%，较少几次即可放大到上限。
pub(crate) const ZOOM_STEPS: [u16; 9] = [70, 80, 90, 100, 110, 125, 150, 175, 200];

/// 设置页中背景图片不透明度每次调整的量（百分比）。
pub(super) const OPACITY_STEP: f32 = 5.0;

/// 设置页中字重每次调整的量。
pub(super) const FONT_WEIGHT_STEP: u16 = 100;

/// 把 `value` 加 1（`up` 为 `true`）或减 1，结果限制在 `range` 之内。
pub(super) fn step(value: f32, up: bool, range: RangeInclusive<f32>) -> f32 {
    let next = if up { value + 1.0 } else { value - 1.0 };
    next.clamp(*range.start(), *range.end())
}

pub mod advisory;
pub(crate) mod checkpoint_io;
pub mod dpc;
pub mod dw;
pub mod remote_common;
pub mod report;
pub mod standalone;
pub mod steps;

/// 执行一个带 checkpoint 跳过逻辑的安装步骤：打印 header/footer；
/// 若 `skip` 为真则打印跳过提示，否则执行 `action`（通常在其内部更新并保存 checkpoint）。
pub async fn run_step<Fut>(
    header: &str,
    skip: bool,
    skip_msg: &str,
    action: Fut,
) -> anyhow::Result<()>
where
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    crate::ui::step_header(header);
    if skip {
        crate::ui::log_info(skip_msg);
    } else {
        action.await?;
    }
    crate::ui::step_footer();
    Ok(())
}

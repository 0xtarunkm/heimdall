use heimdall_plugin::Heimdall;
use solana_geyser_plugin_interface::geyser_plugin_interface::GeyserPlugin;

mod config;
mod heimdall_plugin;
mod models;

#[no_mangle]
#[allow(improper_ctypes_definitions)]
/// # Safety
///
/// The Solana validator and this plugin must be compiled with the same Rust compiler version and Solana core version.
/// Loading this plugin with mismatching versions is undefined behavior and will likely cause memory corruption.
pub unsafe extern "C" fn _create_plugin() -> *mut dyn GeyserPlugin {
    let plugin = Heimdall::default();
    let plugin: Box<dyn GeyserPlugin> = Box::new(plugin);
    Box::into_raw(plugin)
}

//! A minimal clack host for plugins' tests (feature `test-host`): load a
//! [`SilkscreenPlugin`] in-process, as a DAW would, with handlers that
//! accept every request and do nothing, bar recording params rescans
//! ([`rescans`]).

use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
};
use clack_host::factory::plugin::PluginFactory;
use clack_host::prelude::*;
use clack_plugin::entry::SinglePluginEntry;

use crate::{Clap, SilkscreenPlugin};

/// The host's handler types.
#[derive(Debug)]
pub struct Host;

#[derive(Debug)]
pub struct HostShared;

#[derive(Debug, Default)]
pub struct HostMain {
    /// Every `params.rescan` the plugin asked for, in order.
    rescans: Vec<ParamRescanFlags>,
}

#[derive(Debug)]
pub struct HostAudio;

impl SharedHandler<'_> for HostShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

impl AudioProcessorHandler<'_> for HostAudio {}

impl MainThreadHandler<'_> for HostMain {}

impl HostParamsImplShared for HostShared {
    fn request_flush(&self) {}
}

impl HostParamsImplMainThread for HostMain {
    fn rescan(&mut self, flags: ParamRescanFlags) {
        self.rescans.push(flags);
    }

    fn clear(&mut self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

impl HostHandlers for Host {
    type Shared<'a> = HostShared;
    type MainThread<'a> = HostMain;
    type AudioProcessor<'a> = HostAudio;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostParams>();
    }
}

/// Every `params.rescan` `plugin` has asked for, in order.
pub fn rescans(plugin: &PluginInstance<Host>) -> Vec<ParamRescanFlags> {
    plugin.access_handler(|h| h.rescans.clone())
}

/// Plugin `P`'s entry and an instance of its one plugin. Keep the entry
/// alive as long as the instance.
///
/// # Panics
/// If the plugin fails to load or instantiate.
pub fn load<P: SilkscreenPlugin>() -> (PluginEntry, PluginInstance<Host>) {
    let entry = PluginEntry::load_from_clack::<SinglePluginEntry<Clap<P>>>(c"")
        .expect("the plugin's entry loads");
    let plugin = instantiate(&entry);
    (entry, plugin)
}

/// An instance of `entry`'s first plugin, for a test or a smoke run that
/// loaded the entry itself (from a bundle).
///
/// # Panics
/// If the entry has no plugin, or it fails to instantiate.
pub fn instantiate(entry: &PluginEntry) -> PluginInstance<Host> {
    let info = HostInfo::new("silkscreen-test", "", "", "").expect("host info");
    let descriptor = entry
        .get_factory::<PluginFactory>()
        .expect("a plugin factory")
        .plugin_descriptor(0)
        .expect("a plugin");
    let id = descriptor.id().expect("a plugin id").to_owned();
    PluginInstance::<Host>::new(|()| HostShared, |_| HostMain::default(), entry, &id, &info)
        .expect("the plugin instantiates")
}

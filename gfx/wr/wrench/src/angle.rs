/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use glutin::{self, ContextBuilder, ContextCurrentState, CreationError};
use winit::{event_loop::EventLoop, window::Window, window::WindowBuilder};

#[cfg(not(windows))]
pub enum Context {}

#[cfg(windows)]
pub use crate::egl::Context;

impl Context {
    #[cfg(not(windows))]
    pub fn with_window<T: ContextCurrentState>(
        _: WindowBuilder,
        _: ContextBuilder<'_, T>,
        _: &EventLoop<()>,
        _: bool,
    ) -> Result<(Window, Self), CreationError> {
        Err(CreationError::PlatformSpecific(
            "ANGLE rendering is only supported on Windows".into(),
        ))
    }

    #[cfg(windows)]
    pub fn with_window<T: ContextCurrentState>(
        window_builder: WindowBuilder,
        context_builder: ContextBuilder<'_, T>,
        events_loop: &EventLoop<()>,
        using_compositor: bool,
    ) -> Result<(Window, Self), CreationError> {
        use winit::platform::windows::WindowExtWindows;

        // FIXME: &context_builder.pf_reqs  https://github.com/tomaka/glutin/pull/1002
        let pf_reqs = &glutin::PixelFormatRequirements::default();
        let gl_attr = &context_builder.gl_attr.map_sharing(|_| unimplemented!());
        let window = window_builder.build(events_loop)?;
        Self::new(pf_reqs, gl_attr)
            .and_then(|p| p.finish(window.hwnd() as _, using_compositor))
            .map(|context| (window, context))
    }

    #[cfg(not(windows))]
    pub unsafe fn make_current(&self) -> Result<(), glutin::ContextError> {
        match *self {}
    }

    #[cfg(not(windows))]
    pub fn get_proc_address(&self, _: &str) -> *const () {
        match *self {}
    }

    #[cfg(not(windows))]
    pub fn swap_buffers(&self) -> Result<(), glutin::ContextError> {
        match *self {}
    }

    #[cfg(not(windows))]
    pub fn get_api(&self) -> glutin::Api {
        match *self {}
    }
}

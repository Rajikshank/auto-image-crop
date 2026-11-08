//! Choose a hardware DXGI adapter explicitly instead of assuming adapter zero is discrete.
use std::sync::OnceLock;
#[derive(Clone, Debug)]
pub struct Adapter {
    pub index: i32,
    pub name: String,
    pub dedicated_bytes: usize,
}
pub fn preferred() -> Option<&'static Adapter> {
    static ADAPTER: OnceLock<Option<Adapter>> = OnceLock::new();
    ADAPTER
        .get_or_init(|| unsafe {
            use windows::Win32::Graphics::Dxgi::*;
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut choices = Vec::new();
            let mut index = 0;
            while let Ok(adapter) = factory.EnumAdapters1(index) {
                if let Ok(desc) = adapter.GetDesc1()
                    && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
                {
                    let end = desc
                        .Description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.Description.len());
                    choices.push(Adapter {
                        index: index as i32,
                        name: String::from_utf16_lossy(&desc.Description[..end]),
                        dedicated_bytes: desc.DedicatedVideoMemory,
                    });
                }
                index += 1;
            }
            choices.into_iter().max_by_key(|a| a.dedicated_bytes)
        })
        .as_ref()
}

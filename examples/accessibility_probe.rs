use anyhow::{Context, Result, bail};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::w,
};
fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let exe = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/release/objectlift.exe".into());
        let profile = std::env::current_dir()?.join("artifacts/native-accessibility-profile");
        std::fs::create_dir_all(profile.join("ObjectLift"))?;
        std::fs::write(
            profile.join("ObjectLift/settings.json"),
            r#"{"cpu_only":false,"hotkey":"CtrlShiftO","quality_mode":"Fast","quick_copy":false}"#,
        )?;
        let mut child = Command::new(exe)
            .env("LOCALAPPDATA", profile)
            .args(["--demo", "artifacts/truck.jpg"])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let result = (|| -> Result<()> {
            let start = Instant::now();
            let hwnd = loop {
                if child.try_wait()?.is_some() {
                    bail!("Preview exited before UIA verification");
                }
                if let Ok(hwnd) = FindWindowW(w!("ObjectLiftOverlay"), w!("ObjectLift")) {
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    if pid == child.id() {
                        break hwnd;
                    }
                }
                if start.elapsed() > Duration::from_secs(20) {
                    bail!("UI window unavailable");
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            let ui: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            let root = ui.ElementFromHandle(hwnd)?;
            let condition = ui.CreateTrueCondition()?;
            std::thread::sleep(Duration::from_secs(2));
            let elements = root.FindAll(TreeScope_Descendants, &condition)?;
            for i in 0..elements.Length()? {
                let e = elements.GetElement(i)?;
                println!(
                    "name={} type={} focusable={}",
                    e.CurrentName()?,
                    e.CurrentControlType()?.0,
                    e.CurrentIsKeyboardFocusable()?.as_bool()
                );
            }
            let find = |name: &str| -> Result<IUIAutomationElement> {
                let start = Instant::now();
                loop {
                    let elements = root.FindAll(TreeScope_Descendants, &condition)?;
                    for i in 0..elements.Length()? {
                        let e = elements.GetElement(i)?;
                        if e.CurrentName()? == name
                            && e.CurrentControlType()? == UIA_ButtonControlTypeId
                        {
                            return Ok(e);
                        }
                    }
                    if start.elapsed() > Duration::from_secs(10) {
                        bail!("Accessible button missing: {name}");
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            };
            let invoke = |name: &str| -> Result<()> {
                let e = find(name)?;
                if !e.CurrentIsKeyboardFocusable()?.as_bool() {
                    bail!("Button not keyboard focusable: {name}");
                }
                let pattern: IUIAutomationInvokePattern =
                    e.GetCurrentPatternAs(UIA_InvokePatternId)?;
                pattern.Invoke()?;
                Ok(())
            };
            invoke("Select")?;
            // Exercise object selection without pointer input.
            // Discovery is progressive: wait for an available candidate rather than
            // assuming model startup has completed after a fixed two-second delay.
            let start = Instant::now();
            loop {
                PostMessageW(Some(hwnd), WM_KEYDOWN, WPARAM(0x75), LPARAM(0))?;
                std::thread::sleep(Duration::from_millis(200));
                let elements = root.FindAll(TreeScope_Descendants, &condition)?;
                let mut selected = false;
                for i in 0..elements.Length()? {
                    if elements.GetElement(i)?.CurrentName()? == "Inspect" {
                        selected = true;
                        break;
                    }
                }
                if selected {
                    break;
                }
                if start.elapsed() > Duration::from_secs(30) {
                    bail!("Keyboard object selection unavailable after discovery wait");
                }
            }
            invoke("Inspect")?;
            invoke("Refine")?;
            let elements = root.FindAll(TreeScope_Descendants, &condition)?;
            let mut slider = None;
            for i in 0..elements.Length()? {
                let e = elements.GetElement(i)?;
                if e.CurrentControlType()? == UIA_SliderControlTypeId
                    && !e.CurrentIsOffscreen()?.as_bool()
                {
                    slider = Some(e);
                    break;
                }
            }
            let slider = slider.context("Size slider not exposed")?;
            let range: IUIAutomationRangeValuePattern =
                slider.GetCurrentPatternAs(UIA_RangeValuePatternId)?;
            println!(
                "Slider min={} max={} value={} step={}",
                range.CurrentMinimum()?,
                range.CurrentMaximum()?,
                range.CurrentValue()?,
                range.CurrentSmallChange()?
            ); // Native UIA normalizes trackbars to 0..100, with discrete physical steps.
            let before = range.CurrentValue()?;
            let target = (range.CurrentMinimum()? + range.CurrentMaximum()?) / 2.;
            range.SetValue(target)?;
            let start = Instant::now();
            while (range.CurrentValue()? - target).abs() > range.CurrentSmallChange()? + 0.001
                && start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            if range.CurrentValue()? == before
                || (range.CurrentValue()? - target).abs() > range.CurrentSmallChange()? + 0.001
            {
                bail!("Accessible slider did not update within its declared discrete step");
            }
            let native_position =
                SendMessageW(slider.CurrentNativeWindowHandle()?, WM_USER, None, None).0;
            let start = Instant::now();
            while !slider
                .CurrentName()?
                .to_string()
                .contains(&format!("{native_position} "))
                && start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            if !slider
                .CurrentName()?
                .to_string()
                .contains(&format!("{native_position} "))
            {
                bail!(
                    "Accessible brush name '{}' does not reflect actual radius {native_position}",
                    slider.CurrentName()?
                );
            }
            invoke("Done")?;
            let copy = find("Copy")?;
            copy.SetFocus()?;
            if !copy.CurrentHasKeyboardFocus()?.as_bool() {
                bail!("Copy could not receive focus");
            }
            PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
            std::fs::write(
                "artifacts/accessibility-results.json",
                r#"{"result":"PASS","checks":["Keyboard object selection (F6)","Native UI Automation button types and names","InvokePattern","RangeValuePattern","Keyboard focus"],"limitations":"Does not certify physical screen-reader or mixed-DPI operation"}"#,
            )?;
            println!("PASS: native UI Automation names, actions, slider and keyboard focus");
            Ok(())
        })();
        if child.try_wait()?.is_none() {
            if let Ok(hwnd) = FindWindowW(w!("ObjectLiftOverlay"), w!("ObjectLift")) {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            let start = Instant::now();
            while child.try_wait()?.is_none() && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
        }
        CoUninitialize();
        result
    }
}

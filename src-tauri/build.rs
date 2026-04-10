fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .plugin(
                "usb-audio",
                tauri_build::InlinedPlugin::new().commands(&[
                    "listUsbDevices",
                    "requestUsbPermission",
                    "getUsbDeviceInfo",
                    "openUsbDevice",
                    "closeUsbDevice",
                ]),
            )
            .plugin(
                "media-store",
                tauri_build::InlinedPlugin::new().commands(&[
                    "saveToSharedStorage",
                    "saveWavBytes",
                    "createRecordingEntry",
                    "finalizeRecordingEntry",
                    "cancelRecordingEntry",
                    "exportFile",
                ]),
            )
            .plugin(
                "geolocation",
                tauri_build::InlinedPlugin::new().commands(&[
                    "getCurrentLocation",
                    "getWifiSsid",
                    "getDeviceModel",
                ]),
            ),
    )
    .expect("failed to run tauri-build");
}

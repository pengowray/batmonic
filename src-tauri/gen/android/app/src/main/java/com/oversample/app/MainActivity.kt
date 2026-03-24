package com.oversample.app

import android.os.Bundle
import android.util.Log
import androidx.activity.enableEdgeToEdge

private const val TAG = "MainActivity"

class MainActivity : TauriActivity() {
  private var usbAudioPlugin: UsbAudioPlugin? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // Register USB audio plugin before super.onCreate (which initializes the WebView)
    val plugin = UsbAudioPlugin(this)
    usbAudioPlugin = plugin
    pluginManager.load(null, "usb-audio", plugin, "{}")
    super.onCreate(savedInstanceState)
    // Note: We do NOT override the WebChromeClient. The generated RustWebChromeClient
    // already handles RESOURCE_AUDIO_CAPTURE by requesting RECORD_AUDIO + MODIFY_AUDIO_SETTINGS
    // runtime permissions and granting/denying the WebView request based on the result.
  }

  @Suppress("DEPRECATION")
  override fun onRequestPermissionsResult(
    requestCode: Int,
    permissions: Array<out String>,
    grantResults: IntArray
  ) {
    Log.i(TAG, "onRequestPermissionsResult: code=$requestCode, results=${grantResults.toList()}")
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    // Forward to USB audio plugin for RECORD_AUDIO permission handling
    usbAudioPlugin?.handlePermissionResult(requestCode, grantResults)
  }
}

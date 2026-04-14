package com.oversample.app

import android.os.Bundle
import android.util.Log
import androidx.activity.enableEdgeToEdge

private const val TAG = "MainActivity"

class MainActivity : TauriActivity() {
  private var usbAudioPlugin: UsbAudioPlugin? = null
  private var mediaStorePlugin: MediaStorePlugin? = null
  private var geolocationPlugin: GeolocationPlugin? = null

  override fun onWebViewCreate(webView: android.webkit.WebView) {
    // Enable native pinch-to-zoom so users can zoom in to read text
    // and always have a way to zoom back out
    webView.settings.setSupportZoom(true)
    webView.settings.builtInZoomControls = true
    webView.settings.displayZoomControls = false // hide +/- buttons
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // Register plugins before super.onCreate (which initializes the WebView)
    val usbPlugin = UsbAudioPlugin(this)
    usbAudioPlugin = usbPlugin
    pluginManager.load(null, "usb-audio", usbPlugin, "{}")

    val msPlugin = MediaStorePlugin(this)
    mediaStorePlugin = msPlugin
    pluginManager.load(null, "media-store", msPlugin, "{}")

    val geoPlugin = GeolocationPlugin(this)
    geolocationPlugin = geoPlugin
    pluginManager.load(null, "geolocation", geoPlugin, "{}")

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
    // Forward to plugins for permission handling
    usbAudioPlugin?.handlePermissionResult(requestCode, grantResults)
    mediaStorePlugin?.handlePermissionResult(requestCode, grantResults)
    geolocationPlugin?.handlePermissionResult(requestCode, grantResults)
  }

  @Suppress("DEPRECATION")
  override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
    super.onActivityResult(requestCode, resultCode, data)
    // Forward SAF picker results to media store plugin
    mediaStorePlugin?.handleActivityResult(requestCode, resultCode, data)
  }
}

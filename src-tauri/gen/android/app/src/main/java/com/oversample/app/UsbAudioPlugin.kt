package com.oversample.app

import android.Manifest
import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

private const val TAG = "UsbAudioPlugin"
private const val ACTION_USB_PERMISSION = "com.batmonic.app.USB_PERMISSION"

// USB Audio Class constants
private const val USB_CLASS_AUDIO = 1
private const val USB_SUBCLASS_AUDIOCONTROL = 1
private const val USB_SUBCLASS_AUDIOSTREAMING = 2

// CS_INTERFACE descriptor subtypes (Audio Class)
private const val UAC_HEADER = 0x01
private const val UAC1_FORMAT_TYPE = 0x02
private const val UAC2_CLOCK_SOURCE = 0x0A

// USB descriptor types
private const val DESC_TYPE_INTERFACE = 4
private const val DESC_TYPE_ENDPOINT = 5
private const val DESC_TYPE_CS_INTERFACE = 0x24
private const val DESC_TYPE_CS_ENDPOINT = 0x25

// Control transfer constants
private const val DEVICE_TO_HOST_CLASS_INTERFACE = 0xA1
private const val HOST_TO_DEVICE_CLASS_INTERFACE = 0x21
private const val HOST_TO_DEVICE_CLASS_ENDPOINT = 0x22
private const val GET_CUR = 0x01
private const val SET_CUR = 0x01
private const val USB_DIR_OUT_STANDARD_INTERFACE = 0x01
private const val USB_REQUEST_SET_INTERFACE = 11

@TauriPlugin
class UsbAudioPlugin(private val activity: Activity) : Plugin(activity) {

    private var pendingPermissionInvoke: Invoke? = null
    private var pendingPermissionDevice: UsbDevice? = null
    private var pendingAudioPermissionInvoke: Invoke? = null
    private var activeConnection: UsbDeviceConnection? = null
    private var activeDevice: UsbDevice? = null
    private var webViewRef: android.webkit.WebView? = null
    // USB hotplug state: set by BroadcastReceiver, read by checkUsbStatus command
    @Volatile private var lastUsbEvent: String? = null  // "attached" or "detached"
    @Volatile private var lastUsbProductName: String? = null
    @Volatile private var lastUsbDeviceName: String? = null

    companion object {
        const val REQUEST_AUDIO_PERMISSION = 1001
    }

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == ACTION_USB_PERMISSION) {
                // With FLAG_IMMUTABLE, intent extras are empty (no EXTRA_DEVICE or
                // EXTRA_PERMISSION_GRANTED). Use the cached device and check permission
                // directly via UsbManager, matching the batgizmo pattern.
                val device = pendingPermissionDevice
                val invoke = pendingPermissionInvoke
                pendingPermissionInvoke = null
                pendingPermissionDevice = null

                if (invoke == null) return

                val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
                val granted = device != null && usbManager.hasPermission(device)

                if (granted && device != null) {
                    val result = JSObject()
                    result.put("granted", true)
                    result.put("deviceName", device.deviceName)
                    invoke.resolve(result)
                } else {
                    val result = JSObject()
                    result.put("granted", false)
                    invoke.resolve(result)
                }
            }
        }
    }

    // USB hotplug receiver — detects device attach/detach and emits events to frontend
    private val usbHotplugReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val device = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
            }

            val isAudio = device?.let { dev ->
                (0 until dev.interfaceCount).any {
                    dev.getInterface(it).interfaceClass == USB_CLASS_AUDIO
                }
            } ?: false

            if (!isAudio) return

            val action = when (intent.action) {
                UsbManager.ACTION_USB_DEVICE_ATTACHED -> "attached"
                UsbManager.ACTION_USB_DEVICE_DETACHED -> "detached"
                else -> return
            }
            val productName = device?.productName ?: "USB Audio"
            Log.i(TAG, "USB audio device $action: $productName")

            // Store event for polling via checkUsbStatus command
            lastUsbEvent = action
            lastUsbProductName = productName
            lastUsbDeviceName = device?.deviceName ?: ""
        }
    }

    override fun load(webView: android.webkit.WebView) {
        super.load(webView)
        webViewRef = webView

        // Register USB permission receiver
        val permFilter = IntentFilter(ACTION_USB_PERMISSION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.registerReceiver(usbReceiver, permFilter, Context.RECEIVER_EXPORTED)
        } else {
            activity.registerReceiver(usbReceiver, permFilter)
        }

        // Register USB hotplug receiver
        val hotplugFilter = IntentFilter().apply {
            addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.registerReceiver(usbHotplugReceiver, hotplugFilter, Context.RECEIVER_EXPORTED)
        } else {
            activity.registerReceiver(usbHotplugReceiver, hotplugFilter)
        }

        Log.i(TAG, "UsbAudioPlugin loaded")
    }

    /**
     * Request Android RECORD_AUDIO runtime permission.
     * Needed for WebView getUserMedia to work on Android.
     */
    @Command
    fun requestAudioPermission(invoke: Invoke) {
        Log.i(TAG, "requestAudioPermission: checking RECORD_AUDIO")
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.RECORD_AUDIO)
            == PackageManager.PERMISSION_GRANTED) {
            Log.i(TAG, "requestAudioPermission: already granted")
            val result = JSObject()
            result.put("granted", true)
            invoke.resolve(result)
            return
        }

        Log.i(TAG, "requestAudioPermission: requesting from user")
        pendingAudioPermissionInvoke = invoke
        ActivityCompat.requestPermissions(
            activity,
            arrayOf(Manifest.permission.RECORD_AUDIO),
            REQUEST_AUDIO_PERMISSION
        )
    }

    /**
     * Called from MainActivity.onRequestPermissionsResult to resolve pending audio permission.
     */
    fun handlePermissionResult(requestCode: Int, grantResults: IntArray) {
        Log.i(TAG, "handlePermissionResult: code=$requestCode, results=${grantResults.toList()}")
        if (requestCode != REQUEST_AUDIO_PERMISSION) return
        val invoke = pendingAudioPermissionInvoke
        if (invoke == null) {
            Log.w(TAG, "handlePermissionResult: no pending invoke")
            return
        }
        pendingAudioPermissionInvoke = null

        val granted = grantResults.isNotEmpty() &&
                grantResults[0] == PackageManager.PERMISSION_GRANTED
        Log.i(TAG, "handlePermissionResult: granted=$granted")
        val result = JSObject()
        result.put("granted", granted)
        invoke.resolve(result)
    }

    /**
     * Check USB device status: returns whether an audio device is attached,
     * and any pending hotplug event since the last poll.
     * Frontend polls this every few seconds to detect USB connect/disconnect.
     */
    @Command
    fun checkUsbStatus(invoke: Invoke) {
        val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
        val hasAudioDevice = usbManager.deviceList.values.any { device ->
            (0 until device.interfaceCount).any {
                device.getInterface(it).interfaceClass == USB_CLASS_AUDIO
            }
        }

        val result = JSObject()
        result.put("audioDeviceAttached", hasAudioDevice)

        // Report and consume the last hotplug event
        val event = lastUsbEvent
        if (event != null) {
            result.put("lastEvent", event)
            result.put("productName", lastUsbProductName ?: "USB Audio")
            result.put("deviceName", lastUsbDeviceName ?: "")
            lastUsbEvent = null
            lastUsbProductName = null
            lastUsbDeviceName = null
        }

        invoke.resolve(result)
    }

    /**
     * List all connected USB devices with audio class info.
     * Returns basic device info without requiring permission.
     */
    @Command
    fun listUsbDevices(invoke: Invoke) {
        val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
        val deviceList = usbManager.deviceList
        val result = JSObject()
        val devicesArray = JSONArray()

        for ((_, device) in deviceList) {
            val devObj = JSONObject()
            devObj.put("deviceName", device.deviceName)
            devObj.put("vendorId", device.vendorId)
            devObj.put("productId", device.productId)
            devObj.put("productName", device.productName ?: "Unknown")
            devObj.put("manufacturerName", device.manufacturerName ?: "Unknown")
            devObj.put("deviceClass", device.deviceClass)
            devObj.put("hasPermission", usbManager.hasPermission(device))

            // Check if any interface is audio class
            var isAudioDevice = false
            for (i in 0 until device.interfaceCount) {
                val iface = device.getInterface(i)
                if (iface.interfaceClass == USB_CLASS_AUDIO) {
                    isAudioDevice = true
                    break
                }
            }
            devObj.put("isAudioDevice", isAudioDevice)

            devicesArray.put(devObj)
        }

        result.put("devices", devicesArray)
        invoke.resolve(result)
    }

    /**
     * Request USB permission for a device.
     * Takes deviceName as argument.
     */
    @Command
    fun requestUsbPermission(invoke: Invoke) {
        val args = invoke.parseArgs(RequestPermissionArgs::class.java)
        val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
        val device = usbManager.deviceList[args.deviceName]

        if (device == null) {
            invoke.reject("Device not found: ${args.deviceName}")
            return
        }

        if (usbManager.hasPermission(device)) {
            val result = JSObject()
            result.put("granted", true)
            result.put("deviceName", device.deviceName)
            invoke.resolve(result)
            return
        }

        pendingPermissionInvoke = invoke
        pendingPermissionDevice = device

        val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            PendingIntent.FLAG_IMMUTABLE
        } else {
            0
        }
        val permissionIntent = PendingIntent.getBroadcast(activity, 0,
            Intent(ACTION_USB_PERMISSION), flags)
        usbManager.requestPermission(device, permissionIntent)
    }

    /**
     * Get detailed audio info for a USB device by parsing its raw descriptors.
     * Requires permission. Returns supported sample rates, channels, bit depth, UAC version.
     */
    @Command
    fun getUsbDeviceInfo(invoke: Invoke) {
        val args = invoke.parseArgs(DeviceNameArgs::class.java)
        val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
        val device = usbManager.deviceList[args.deviceName]

        if (device == null) {
            invoke.reject("Device not found: ${args.deviceName}")
            return
        }

        if (!usbManager.hasPermission(device)) {
            invoke.reject("No permission for device: ${args.deviceName}")
            return
        }

        val connection = usbManager.openDevice(device)
        if (connection == null) {
            invoke.reject("Failed to open device: ${args.deviceName}")
            return
        }

        try {
            // Claim all audio interfaces so we can read UAC2 clock source
            for (i in 0 until device.interfaceCount) {
                val iface = device.getInterface(i)
                if (iface.interfaceClass == USB_CLASS_AUDIO) {
                    connection.claimInterface(iface, true)
                }
            }

            val rawDescriptors = connection.rawDescriptors
            val audioInfo = parseAudioDescriptors(rawDescriptors, connection)

            val result = JSObject()
            result.put("deviceName", device.deviceName)
            result.put("vendorId", device.vendorId)
            result.put("productId", device.productId)
            result.put("productName", device.productName ?: "Unknown")
            result.put("manufacturerName", device.manufacturerName ?: "Unknown")
            result.put("uacVersion", audioInfo.uacVersion)
            result.put("fileDescriptor", connection.fileDescriptor)

            val ratesArray = JSONArray()
            for (rate in audioInfo.sampleRates) {
                ratesArray.put(rate)
            }
            result.put("sampleRates", ratesArray)

            val endpointsArray = JSONArray()
            for (ep in audioInfo.endpoints) {
                val epObj = JSONObject()
                epObj.put("address", ep.address)
                epObj.put("maxPacketSize", ep.maxPacketSize)
                epObj.put("channels", ep.channels)
                epObj.put("bitResolution", ep.bitResolution)
                epObj.put("sampleRate", ep.sampleRate)
                epObj.put("sampleRateSettable", ep.sampleRateSettable)
                epObj.put("interfaceNumber", ep.interfaceNumber)
                epObj.put("alternateSetting", ep.alternateSetting)
                endpointsArray.put(epObj)
            }
            result.put("endpoints", endpointsArray)

            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e(TAG, "Error parsing USB descriptors", e)
            invoke.reject("Error parsing USB descriptors: ${e.message}")
        } finally {
            // Release interfaces but keep connection open (fd may be used later)
            for (i in 0 until device.interfaceCount) {
                val iface = device.getInterface(i)
                if (iface.interfaceClass == USB_CLASS_AUDIO) {
                    connection.releaseInterface(iface)
                }
            }
            connection.close()
        }
    }

    /**
     * Open a USB audio device for isochronous streaming.
     * Claims the audio interface, sets the alternate setting,
     * configures the sample rate, and returns the fd + endpoint info.
     * The connection is kept open until closeUsbDevice is called.
     */
    @Command
    fun openUsbDevice(invoke: Invoke) {
        val args = invoke.parseArgs(OpenUsbDeviceArgs::class.java)
        val usbManager = activity.getSystemService(Context.USB_SERVICE) as UsbManager
        val device = usbManager.deviceList[args.deviceName]

        if (device == null) {
            invoke.reject("Device not found: ${args.deviceName}")
            return
        }

        if (!usbManager.hasPermission(device)) {
            invoke.reject("No permission for device: ${args.deviceName}")
            return
        }

        // Close any previous connection
        activeConnection?.close()
        activeConnection = null
        activeDevice = null

        val connection = usbManager.openDevice(device)
        if (connection == null) {
            invoke.reject("Failed to open device: ${args.deviceName}")
            return
        }

        try {
            // Claim all audio interfaces
            for (i in 0 until device.interfaceCount) {
                val iface = device.getInterface(i)
                if (iface.interfaceClass == USB_CLASS_AUDIO) {
                    connection.claimInterface(iface, true)
                }
            }

            // Parse descriptors to find the best endpoint
            val rawDescriptors = connection.rawDescriptors
            val audioInfo = parseAudioDescriptors(rawDescriptors, connection)

            if (audioInfo.endpoints.isEmpty()) {
                connection.close()
                invoke.reject("No audio input endpoints found")
                return
            }

            // Select the best endpoint for the requested sample rate
            val desiredRate = if (args.sampleRate > 0) args.sampleRate else 384000
            val endpoint = audioInfo.endpoints
                .filter { it.address and 0x80 != 0 } // input endpoints only
                .maxByOrNull { it.sampleRate }
                ?: audioInfo.endpoints.first()

            Log.i(TAG, "Selected endpoint: addr=0x${endpoint.address.toString(16)} " +
                    "maxPkt=${endpoint.maxPacketSize} rate=${endpoint.sampleRate} " +
                    "ch=${endpoint.channels} bits=${endpoint.bitResolution} " +
                    "iface=${endpoint.interfaceNumber} alt=${endpoint.alternateSetting}")

            // Activate the correct alternate setting via the Android API.
            // This uses USBDEVFS_SETINTERFACE ioctl under the hood, which properly
            // updates the kernel's endpoint tables. A raw controlTransfer for
            // SET_INTERFACE does NOT always update the kernel state, causing
            // USBDEVFS_SUBMITURB to fail with ENOENT.
            val targetIface = findUsbInterface(device,
                endpoint.interfaceNumber, endpoint.alternateSetting)
            if (targetIface != null) {
                val ok = connection.setInterface(targetIface)
                Log.i(TAG, "setInterface(iface=${endpoint.interfaceNumber}, " +
                        "alt=${endpoint.alternateSetting}) result=$ok")
                if (!ok) {
                    Log.w(TAG, "setInterface failed — falling back to controlTransfer")
                    val setAltResult = connection.controlTransfer(
                        USB_DIR_OUT_STANDARD_INTERFACE,  // 0x01
                        USB_REQUEST_SET_INTERFACE,        // 11 (SET_INTERFACE)
                        endpoint.alternateSetting,
                        endpoint.interfaceNumber,
                        null, 0, 1000
                    )
                    Log.d(TAG, "controlTransfer SET_INTERFACE result=$setAltResult")
                }
            } else if (endpoint.alternateSetting > 0) {
                // UsbInterface object not found — fall back to controlTransfer
                Log.w(TAG, "UsbInterface not found for iface=${endpoint.interfaceNumber} " +
                        "alt=${endpoint.alternateSetting}, using controlTransfer fallback")
                val setAltResult = connection.controlTransfer(
                    USB_DIR_OUT_STANDARD_INTERFACE,
                    USB_REQUEST_SET_INTERFACE,
                    endpoint.alternateSetting,
                    endpoint.interfaceNumber,
                    null, 0, 1000
                )
                Log.d(TAG, "controlTransfer SET_INTERFACE result=$setAltResult")
            }

            // Set sample rate via control transfer
            val actualRate = if (desiredRate > 0 && endpoint.sampleRateSettable) {
                setSampleRate(connection, audioInfo.uacVersion, endpoint, desiredRate)
            } else {
                endpoint.sampleRate
            }

            // Keep connection open for streaming
            activeConnection = connection
            activeDevice = device

            val result = JSObject()
            result.put("fd", connection.fileDescriptor)
            result.put("endpointAddress", endpoint.address and 0x7F) // strip direction bit
            result.put("maxPacketSize", endpoint.maxPacketSize)
            result.put("sampleRate", actualRate)
            result.put("numChannels", endpoint.channels)
            result.put("bitResolution", endpoint.bitResolution)
            result.put("interfaceNumber", endpoint.interfaceNumber)
            result.put("alternateSetting", endpoint.alternateSetting)
            result.put("deviceName", device.deviceName)
            result.put("productName", device.productName ?: "Unknown")
            result.put("uacVersion", audioInfo.uacVersion)
            invoke.resolve(result)

        } catch (e: Exception) {
            Log.e(TAG, "Error opening USB device", e)
            connection.close()
            invoke.reject("Error opening USB device: ${e.message}")
        }
    }

    /**
     * Close the active USB device connection.
     */
    @Command
    fun closeUsbDevice(invoke: Invoke) {
        try {
            activeDevice?.let { device ->
                activeConnection?.let { conn ->
                    for (i in 0 until device.interfaceCount) {
                        val iface = device.getInterface(i)
                        if (iface.interfaceClass == USB_CLASS_AUDIO) {
                            conn.releaseInterface(iface)
                        }
                    }
                    conn.close()
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "Error closing USB device: ${e.message}")
        }
        activeConnection = null
        activeDevice = null
        invoke.resolve(JSObject())
    }

    /**
     * Find the Android UsbInterface object matching a specific interface number
     * and alternate setting. Needed for connection.setInterface() which requires
     * the actual UsbInterface object, not just raw numbers.
     */
    private fun findUsbInterface(
        device: UsbDevice,
        interfaceNumber: Int,
        alternateSetting: Int
    ): android.hardware.usb.UsbInterface? {
        for (i in 0 until device.interfaceCount) {
            val iface = device.getInterface(i)
            if (iface.id == interfaceNumber && iface.alternateSetting == alternateSetting) {
                return iface
            }
        }
        return null
    }

    /**
     * Set the sample rate on a USB audio device.
     * UAC1: SET_CUR to endpoint with 3-byte LE rate.
     * UAC2: SET_CUR to clock source with 4-byte LE rate.
     */
    private fun setSampleRate(
        connection: UsbDeviceConnection,
        uacVersion: Int,
        endpoint: AudioEndpointInfo,
        desiredRate: Int
    ): Int {
        if (uacVersion == 2) {
            // UAC2: SET_CUR to clock source
            // We need the clock ID — try to read it again from descriptors
            val rawDesc = connection.rawDescriptors
            val clockId = findUac2ClockId(rawDesc)
            if (clockId >= 0) {
                val buffer = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN)
                buffer.putInt(desiredRate)
                val data = buffer.array()
                val result = connection.controlTransfer(
                    HOST_TO_DEVICE_CLASS_INTERFACE,  // 0x21
                    SET_CUR,                          // 0x01
                    0x0100,                           // CS_SAM_FREQ_CONTROL << 8
                    clockId shl 8,                    // clock ID in high byte
                    data, data.size, 1000
                )
                Log.i(TAG, "UAC2 SET_CUR rate=$desiredRate clockId=$clockId result=$result")
                if (result >= 0) {
                    // Verify the rate was set
                    try {
                        val actual = getUac2SampleRate(connection, clockId)
                        if (actual > 0) return actual
                    } catch (_: Exception) {}
                    return desiredRate
                }
            }
        } else {
            // UAC1: SET_CUR to endpoint
            val data = ByteArray(3)
            data[0] = (desiredRate and 0xFF).toByte()
            data[1] = ((desiredRate shr 8) and 0xFF).toByte()
            data[2] = ((desiredRate shr 16) and 0xFF).toByte()
            val result = connection.controlTransfer(
                HOST_TO_DEVICE_CLASS_ENDPOINT,  // 0x22
                SET_CUR,                         // 0x01
                0x0100,                          // SAMPLING_FREQ_CONTROL << 8
                endpoint.address,                // endpoint address
                data, data.size, 1000
            )
            Log.i(TAG, "UAC1 SET_CUR rate=$desiredRate ep=0x${endpoint.address.toString(16)} result=$result")
            if (result >= 0) return desiredRate
        }
        // Fallback to the endpoint's reported rate
        return endpoint.sampleRate
    }

    /** Find UAC2 clock source ID from raw descriptors. */
    private fun findUac2ClockId(raw: ByteArray): Int {
        var offset = 0
        while (offset < raw.size) {
            val bLength = raw[offset].toInt() and 0xFF
            if (bLength < 2 || offset + bLength > raw.size) break
            val bDescriptorType = raw[offset + 1].toInt() and 0xFF
            if (bDescriptorType == DESC_TYPE_CS_INTERFACE && bLength >= 8) {
                val bDescriptorSubtype = raw[offset + 2].toInt() and 0xFF
                if (bDescriptorSubtype == UAC2_CLOCK_SOURCE) {
                    val bClockId = raw[offset + 3].toInt() and 0xFF
                    val bmAttributes = raw[offset + 4].toInt() and 0xFF
                    val bmControls = raw[offset + 5].toInt() and 0xFF
                    if (bmAttributes and 0x03 != 0 && bmControls and 0x01 != 0) {
                        return bClockId
                    }
                }
            }
            offset += bLength
        }
        return -1
    }

    // ── Raw descriptor parsing ───────────────────────────────────────────

    data class AudioEndpointInfo(
        val address: Int,
        val maxPacketSize: Int,
        val channels: Int,
        val bitResolution: Int,
        val sampleRate: Int,
        val sampleRateSettable: Boolean,
        val interfaceNumber: Int,
        val alternateSetting: Int
    )

    data class AudioDeviceInfo(
        val uacVersion: Int,  // 1 or 2
        val sampleRates: List<Int>,
        val endpoints: List<AudioEndpointInfo>
    )

    /**
     * Parse raw USB descriptors to find audio streaming interfaces and their capabilities.
     * Handles both UAC1 (sample rates in format descriptor) and UAC2 (clock source).
     */
    private fun parseAudioDescriptors(
        raw: ByteArray,
        connection: UsbDeviceConnection
    ): AudioDeviceInfo {
        var uacVersion = 0
        var uac2ClockId = -1
        val sampleRates = mutableSetOf<Int>()
        val endpoints = mutableListOf<AudioEndpointInfo>()

        // State for tracking current interface/format being parsed
        var currentInterfaceNumber = -1
        var currentAlternateSetting = 0
        var currentChannels = 1
        var currentBitResolution = 16
        var currentSampleRate = 0
        var foundAudioStreaming = false
        var expectingFormat = false
        var expectingEndpoint = false
        var isUac2Format = false
        var sampleRateSettable = false
        var lastEndpointAddress = 0
        var lastMaxPacketSize = 0

        var offset = 0
        while (offset < raw.size) {
            val bLength = raw[offset].toInt() and 0xFF
            if (bLength < 2 || offset + bLength > raw.size) break

            val bDescriptorType = raw[offset + 1].toInt() and 0xFF

            when (bDescriptorType) {
                DESC_TYPE_INTERFACE -> {
                    if (bLength >= 9) {
                        val bInterfaceNumber = raw[offset + 2].toInt() and 0xFF
                        val bAlternateSetting = raw[offset + 3].toInt() and 0xFF
                        val bNumEndpoints = raw[offset + 4].toInt() and 0xFF
                        val bInterfaceClass = raw[offset + 5].toInt() and 0xFF
                        val bInterfaceSubClass = raw[offset + 6].toInt() and 0xFF

                        currentInterfaceNumber = bInterfaceNumber
                        currentAlternateSetting = bAlternateSetting

                        if (bInterfaceClass == USB_CLASS_AUDIO && bInterfaceSubClass == USB_SUBCLASS_AUDIOSTREAMING && bNumEndpoints > 0) {
                            foundAudioStreaming = true
                            expectingFormat = true
                            isUac2Format = false
                        } else {
                            foundAudioStreaming = false
                            expectingFormat = false
                        }
                        expectingEndpoint = false
                    }
                }

                DESC_TYPE_CS_INTERFACE -> {
                    if (bLength >= 3) {
                        val bDescriptorSubtype = raw[offset + 2].toInt() and 0xFF

                        // UAC Header — detect version
                        if (bDescriptorSubtype == UAC_HEADER && bLength >= 6) {
                            // Check which interface class this belongs to
                            val bcdADC = (raw[offset + 4].toInt() and 0xFF) or
                                    ((raw[offset + 3].toInt() and 0xFF) shl 8)
                            if (bcdADC >= 0x0200) {
                                uacVersion = 2
                            } else if (uacVersion == 0) {
                                uacVersion = 1
                            }
                            Log.d(TAG, "UAC header version: $bcdADC -> UAC$uacVersion")
                        }

                        // UAC2 Clock Source (subtype 0x0A)
                        if (bDescriptorSubtype == UAC2_CLOCK_SOURCE && bLength >= 8) {
                            val bClockId = raw[offset + 3].toInt() and 0xFF
                            val bmAttributes = raw[offset + 4].toInt() and 0xFF
                            val bmControls = raw[offset + 5].toInt() and 0xFF
                            // bmAttributes==1: internal fixed clock, bmControls bit 0: freq readable
                            if (bmAttributes and 0x03 != 0 && bmControls and 0x01 != 0) {
                                uac2ClockId = bClockId
                                Log.d(TAG, "UAC2 clock source found: id=$bClockId")
                            }
                        }

                        // UAC1 AS General (subtype 0x01 under audio streaming)
                        if (foundAudioStreaming && bDescriptorSubtype == 0x01 && expectingFormat) {
                            if (bLength >= 7) {
                                // AS_GENERAL: check formatTag for PCM (1)
                                val wFormatTag = (raw[offset + 5].toInt() and 0xFF) or
                                        ((raw[offset + 6].toInt() and 0xFF) shl 8)
                                if (wFormatTag == 1) {
                                    // PCM format — continue to format type descriptor
                                    Log.d(TAG, "Audio streaming PCM format found")
                                }
                            }
                        }

                        // UAC1 Format Type I (subtype 0x02 under audio streaming)
                        if (foundAudioStreaming && bDescriptorSubtype == UAC1_FORMAT_TYPE && expectingFormat) {
                            if (bLength >= 8) {
                                val bFormatType = raw[offset + 3].toInt() and 0xFF
                                if (bFormatType == 1) {  // FORMAT_TYPE_I
                                    currentChannels = raw[offset + 4].toInt() and 0xFF
                                    // bSubframeSize at offset+5
                                    currentBitResolution = raw[offset + 6].toInt() and 0xFF
                                    val bSamFreqType = raw[offset + 7].toInt() and 0xFF

                                    if (bSamFreqType == 0 && bLength >= 14) {
                                        // Continuous range: tLowerSamFreq, tUpperSamFreq (3 bytes each)
                                        val lower = read3ByteLE(raw, offset + 8)
                                        val upper = read3ByteLE(raw, offset + 11)
                                        Log.d(TAG, "UAC1 continuous rate: $lower - $upper Hz")
                                        sampleRates.add(lower)
                                        sampleRates.add(upper)
                                        // Add common rates in range
                                        for (r in intArrayOf(44100, 48000, 96000, 192000, 256000, 384000, 500000)) {
                                            if (r in lower..upper) sampleRates.add(r)
                                        }
                                        currentSampleRate = upper
                                    } else {
                                        // Discrete sample rates (3 bytes each)
                                        for (i in 0 until bSamFreqType) {
                                            val rateOffset = offset + 8 + (i * 3)
                                            if (rateOffset + 3 <= raw.size) {
                                                val rate = read3ByteLE(raw, rateOffset)
                                                sampleRates.add(rate)
                                                Log.d(TAG, "UAC1 discrete rate: $rate Hz")
                                                if (rate > currentSampleRate) {
                                                    currentSampleRate = rate
                                                }
                                            }
                                        }
                                    }
                                    expectingFormat = false
                                    expectingEndpoint = true
                                }
                            }
                        }

                        // UAC2 AS General (subtype 0x01 under audio streaming, UAC2)
                        if (foundAudioStreaming && uacVersion == 2 && bDescriptorSubtype == 0x01 && expectingFormat) {
                            // For UAC2, format type descriptor follows
                            isUac2Format = true
                        }

                        // UAC2 Format Type I (subtype 0x02 under audio streaming, UAC2)
                        if (foundAudioStreaming && uacVersion == 2 && bDescriptorSubtype == UAC1_FORMAT_TYPE && isUac2Format) {
                            if (bLength >= 6) {
                                val bFormatType = raw[offset + 3].toInt() and 0xFF
                                if (bFormatType == 1) {
                                    // UAC2 format type I: bSubSlotSize at offset+4, bBitResolution at offset+5
                                    currentBitResolution = raw[offset + 5].toInt() and 0xFF
                                    val subSlotSize = raw[offset + 4].toInt() and 0xFF
                                    currentChannels = 1  // typically mono for bat mics

                                    // UAC2: sample rate comes from clock source, not format descriptor
                                    if (uac2ClockId >= 0) {
                                        try {
                                            val rate = getUac2SampleRate(connection, uac2ClockId)
                                            if (rate > 0) {
                                                currentSampleRate = rate
                                                sampleRates.add(rate)
                                            }
                                        } catch (e: Exception) {
                                            Log.w(TAG, "Failed to read UAC2 sample rate: ${e.message}")
                                        }
                                    }

                                    expectingFormat = false
                                    isUac2Format = false
                                    expectingEndpoint = true
                                }
                            }
                        }
                    }
                }

                DESC_TYPE_ENDPOINT -> {
                    if (bLength >= 7 && expectingEndpoint) {
                        val bEndpointAddress = raw[offset + 2].toInt() and 0xFF
                        val bmAttributes = raw[offset + 3].toInt() and 0xFF
                        val wMaxPacketSize = (raw[offset + 4].toInt() and 0xFF) or
                                ((raw[offset + 5].toInt() and 0xFF) shl 8)

                        // Check: input direction (bit 7 set) + isochronous transfer (bits 0-1 == 1)
                        val isInput = (bEndpointAddress and 0x80) != 0
                        val isIsochronous = (bmAttributes and 0x03) == 1

                        if (isInput && isIsochronous) {
                            Log.d(TAG, "Audio endpoint: addr=0x${bEndpointAddress.toString(16)} maxPkt=$wMaxPacketSize")
                            lastEndpointAddress = bEndpointAddress
                            lastMaxPacketSize = wMaxPacketSize
                            expectingEndpoint = false
                        }
                    }
                }

                DESC_TYPE_CS_ENDPOINT -> {
                    // Audio class endpoint descriptor — check if sample rate is settable
                    if (bLength >= 4) {
                        val bmAttr = raw[offset + 3].toInt() and 0xFF
                        sampleRateSettable = (bmAttr and 0x01) != 0

                        // This completes an endpoint discovery — record it
                        if (currentSampleRate > 0) {
                            endpoints.add(AudioEndpointInfo(
                                address = lastEndpointAddress,
                                maxPacketSize = lastMaxPacketSize,
                                channels = currentChannels,
                                bitResolution = currentBitResolution,
                                sampleRate = currentSampleRate,
                                sampleRateSettable = sampleRateSettable,
                                interfaceNumber = currentInterfaceNumber,
                                alternateSetting = currentAlternateSetting
                            ))
                        }
                    }
                }
            }

            offset += bLength
        }

        // If we found a UAC2 clock source but haven't read its rate yet, try now
        if (uac2ClockId >= 0 && sampleRates.isEmpty()) {
            try {
                val rate = getUac2SampleRate(connection, uac2ClockId)
                if (rate > 0) sampleRates.add(rate)
            } catch (e: Exception) {
                Log.w(TAG, "Failed to read UAC2 sample rate: ${e.message}")
            }
        }

        return AudioDeviceInfo(
            uacVersion = if (uacVersion == 0) 1 else uacVersion,
            sampleRates = sampleRates.sorted(),
            endpoints = endpoints
        )
    }

    /**
     * Read UAC2 sample rate via GET_CUR control transfer to clock source.
     */
    private fun getUac2SampleRate(connection: UsbDeviceConnection, clockId: Int): Int {
        val buffer = ByteArray(4)
        val csSamFreqControl = 1

        val result = connection.controlTransfer(
            DEVICE_TO_HOST_CLASS_INTERFACE,  // 0xA1
            GET_CUR,                         // 0x01
            csSamFreqControl shl 8,          // 0x0100
            clockId shl 8,                   // clock ID in high byte
            buffer,
            buffer.size,
            1000
        )

        if (result != 4) {
            throw RuntimeException("UAC2 GET_CUR failed: result=$result (expected 4)")
        }

        val sampleRate = ByteBuffer.wrap(buffer).order(ByteOrder.LITTLE_ENDIAN).int
        Log.i(TAG, "UAC2 sample rate: $sampleRate Hz")
        return sampleRate
    }

    /** Read a 3-byte little-endian integer (used for UAC1 sample rates). */
    private fun read3ByteLE(data: ByteArray, offset: Int): Int {
        return (data[offset].toInt() and 0xFF) or
                ((data[offset + 1].toInt() and 0xFF) shl 8) or
                ((data[offset + 2].toInt() and 0xFF) shl 16)
    }
}

// Argument data classes for Tauri command deserialization
@InvokeArg
data class RequestPermissionArgs(val deviceName: String = "")

@InvokeArg
data class DeviceNameArgs(val deviceName: String = "")

@InvokeArg
data class OpenUsbDeviceArgs(val deviceName: String = "", val sampleRate: Int = 0)

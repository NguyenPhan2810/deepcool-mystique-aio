# Hardware identity and USB enumeration facts

Captured from the live machine on 2026-11-17, read-only, without opening the device.
This is the descriptor-level ground truth a Linux driver must match. No device I/O was
performed to produce this file.

## PnP identity

```
InstanceId            USB\VID_3633&PID_0009\<serial>
HardwareIds           USB\VID_3633&PID_0009&REV_0246
                      USB\VID_3633&PID_0009
CompatibleIds         USB\MS_COMP_WINUSB
                      USB\COMPAT_VID_3633&Class_FF&SubClass_00&Prot_00
                      ...
                      USB\Class_FF
Service               WINUSB
BusReportedDeviceDesc MYSTIQUE
FriendlyName          MYSTIQUE
Status                OK
```

Command used:

```powershell
$id = 'USB\VID_3633&PID_0009\<serial>'
Get-PnpDeviceProperty -InstanceId $id -KeyName DEVPKEY_Device_CompatibleIds
```

## What this settles for Linux

1. **The device is USB class `0xFF`, subclass `0x00`, protocol `0x00`** — vendor-specific,
   interface-level. It is not HID, not mass storage, not CDC, not audio.

   Consequence: **`hidraw` does not apply.** There is no HID report descriptor to parse and
   no `hid-generic` binding. On Linux the device will enumerate with `usbfs` and be
   reachable through `libusb` with no kernel driver involved. A userspace driver is the
   correct architecture, not a kernel module.

2. **`USB\MS_COMP_WINUSB` is present.** The device advertises the Microsoft OS 1.0
   descriptor set, which is *why* Windows binds WinUSB through a generic compatible ID
   rather than needing a bespoke INF for the endpoints. Linux ignores this descriptor; it
   changes nothing about the interface, but it confirms the vendor designed the device for
   raw vendor-class access rather than for any standard class driver.

3. **`REV_0246`** is the reported device revision (`bcdDevice` 0x0246). Worth keying any
   future model table on, since sibling LCD families likely differ here.

4. **Enumeration topology**: `PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(13)#USB(1)` — the panel
   sits behind a root hub on the XHCI controller, one hub level deep. In this topology it
   is a high-speed-capable port but the device itself negotiates full speed, which matches
   the observed ~12 Mbps ceiling and explains why a 48-byte frame per second is negligible
   bandwidth.

5. **Power**: the descriptor reports a 25 mA draw (`DEVPKEY_Device_PowerData`), i.e. bus
   powered for the *logic* only. The pump and the LCD backlight are fed from the SATA/PSU
   harness, not the USB rail. This matters operationally: **cutting USB power or suspending
   the port does not stop the pump**, and conversely a wedged USB link is recoverable by a
   port reset without endangering the cooler. This is the safest possible failure mode and
   should be stated in the Linux driver's README.

## Facts still unknown at the descriptor level

- `bNumInterfaces` and whether interface 0 is the only interface. The Windows code queries
  interface 0 and finds exactly four endpoints (`0x01`, `0x81`, `0x02`, `0x82`), which is
  consistent with a single interface, but this was never printed from a descriptor dump.
- `bMaxPacketSize0` and whether the four endpoints are all bulk (the code assumes bulk and
  sets bulk pipe policy, and it works, so this is effectively proven by behaviour).
- Whether the device exposes more than one configuration.

These are cheap to confirm on Linux on day one with `lsusb -v -d 3633:0009`, and should be
the first command run after the switch. No Windows-side work is needed to answer them.

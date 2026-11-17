# Troubleshooting

Start with the section that matches what you see. If you're still stuck, contact support.

## The ZED Box Mini doesn't turn on

### Check the power

1. Unplug the power cable.
2. Check with a voltmeter that the supply gives 12 V.
3. If it does, plug the cable back in. If it doesn't, contact support.

### Check the power LED

- LED on: the hardware has power, so TekaOS is the problem. See TekaOS doesn't start, below.
- LED off: if the power checks out, the hardware may have failed. Contact support.

## TekaOS doesn't start

1. Restart the box. If an update failed, it goes back to the last version that worked.
2. Press Space while it starts and pick an older version from the boot menu.
3. If nothing works, reinstall TekaOS. See [Reset and Update](05_reset-to-factory-settings.md).

> **Tip:**
> Shut down from System Update before you unplug the power. Pulling the power while TekaOS runs can damage your data.

## I can't open the web interface

- Check that the ethernet LEDs are lit and your computer is on the same network.
- Give the box a couple of minutes after you power it on.
- On Linux, install libnss-mdns. On Windows, set your network to Private. Both are needed to find .local names.
- Renamed the box? Its address is now `teka-<your-name>.local`. Run `teka box list --discover` to list every TekaOS box on your network.
- Look up the box's IP address in your router's list of connected devices and go to `http://<ip-address>` instead.
## I forgot the admin password

A factory reset clears the admin password. Open General, under System Utilities, and start Factory reset. See [Reset to factory settings](05_reset-to-factory-settings.md). When the box is back, create a new password at `http://teka-<serial>.local/login`.

If you can't open the web interface, contact Ouster support. See [Contact support](#contact-support).

## I can't install apps

- Set an administrator password before installing: set a password first. See [step 06 in Installation](../01_getting-started/02_connect-your-zed-box.md#06b-create-an-admin-password).
- teka can't sign in: run `teka box select teka-<serial>` again to replace the saved password. If TEKA_PASSWORD is set, teka uses it instead of the saved password, so check that it's current.
- teka asks for a box: run `teka box select` first, or add `--box http://teka-<serial>.local/` to the command. System commands need one box, so pass `--box` when you've selected several.
- invalid_image: the app was built for another box type. Deploy with --build, which builds for the box you're deploying to. If you use teka app build, add --arch aarch64.
- teka app list doesn't show your app: run it from your app's folder. To see the starter examples, run `teka app list --examples`.
- teka: command not found: open a new terminal. If that doesn't help, run the install command from step 01 of [Deploy your first App](../01_getting-started/03_deploy-your-first-app.md) again.

## An update didn't finish

If a service fails after an update, System Update lists the failed services. Open Apps, stop or remove the app that's failing, then go back to System Update and click Retry committing update. If you do nothing, the next restart takes the box back to the previous version.

## The ZED Box Mini loses the date and time

If the clock is wrong after the box has been unplugged for a few days with no internet, the clock battery is flat. TekaOS sets the time from the internet whenever it can reach it. To replace the battery, you'll need a CR1220 3 V coin cell, a Torx T6 screwdriver, and a small flat tool.

> **Note:**
> For more detailed instructions including photos of this process, refer to the stereolabs documentation page [[link]](https://docs.stereolabs.com/docs/products/embedded/zed-box-series/zed-box-mini/troubleshooting)

## The screen stays blank

That's normal. TekaOS has no desktop, so the screen only shows something when an app uses it.

## Contact support

Still stuck? Contact Ouster support at [TBD: support contact]. Include your box's serial number and what you've tried.

# Reset your ZED Box Mini to factory settings

A factory reset erases your settings and apps, so your ZED Box starts up like it did out of the box. TekaOS stays installed, at the version you're running now.

> **Warning:**
> You can't undo a factory reset. Copy anything you want to keep off the box first.

## What a reset erases

- Your admin password. The password teka saved for this box stops working.
- Your custom hostname. The box goes back to `teka-<serial>.local.`
- Network settings, such as a static address or Wi-Fi.
- Every installed app and its data, including system apps like Sensors.
- Logs.

TekaOS itself, the version before it, and your box's serial number stay.

## 01. Start the reset

1. Open the web interface at `http://teka-<serial>.local` and sign in.
2. Open General, under System Utilities.
3. In the Factory reset card, click Factory reset.
4. Click Factory reset again to confirm.

![01. Start the reset](https://storage.googleapis.com/docs-ouster-com/teka-docs/0.1.0/images/factory-reset.png)

The box restarts and wipes its settings while it starts up. Don't power it off. When it's back, the page shows Box online or takes you to the sign-in page. The same action is `POST /api/v1/host/factory-reset`.

## 02. Set up your box again

Your box now has no password, so you'll set it up like a new one:

1. Create a new admin password at `http://teka-<serial>.local/login`. See [06b: Create an admin password](../01_getting-started/02_connect-your-zed-box.md#06b-create-an-admin-password).
2. Rename the box and set its network settings again, if you'd changed them.
3. Sign in with teka again. Your old saved password no longer works:

```bash
teka box select teka-<serial>
```

Then reinstall your apps with `teka app deploy`. See [Deploy your first App](../01_getting-started/03_deploy-your-first-app.md).

> **Note:**
> TekaOS 0.1 doesn't put the system apps back after a reset. Contact Ouster support to reinstall them.

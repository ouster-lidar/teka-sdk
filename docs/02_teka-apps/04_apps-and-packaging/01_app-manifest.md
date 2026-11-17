# App Manifest

The manifest puts your app in the TekaOS web interface. It adds a sidebar entry, serves your web pages, and sends API calls to your app. No manifest? Your app still runs. It just doesn't get a web page.

## Add a manifest

Add a `manifest.json` file to your app folder, and pass it to the build as `appManifest` in `default.nix`. Put static web files in a `web/` folder, and list each one in `webFiles`.

```json
{
  "id": "hello-http",
  "version": "1.0.0",
  "display_name": "Hello HTTP",
  "nav": { "label": "Hello HTTP", "path": "/app/hello-http" },
  "ui": { "mode": "external", "external_port": 8000 },
  "backend": { "mode": "none" }
}
```

## Fields

| Field | What it does |
| --- | --- |
| id | Your app ID. It must match the image. |
| version | Your app version. |
| display_name | The name people see. |
| nav | The sidebar entry: a label, and a path (always `/app/\<id>`). |
| ui | static shows your web/ files inside TekaOS. external opens a port on the box. |
| backend | none, or proxy to send `/api/v1/app/\<id>/` requests to your app. |
| auth | Set to ojs-proxy-identity to see who's calling. Learn more in [Box Resources and Firewall](03_box-resources-and-firewall.md). |
| app_cards | Optional status cards. The web interface does not show them. |

> **Note:**
> Bundle your fonts and scripts with the app. Boxes are often offline.

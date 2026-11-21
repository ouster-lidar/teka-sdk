# App Manifest

Every app has a `manifest.json` with its ID and release version. Apps with a `nav` section appear in the TekaOS web interface.

## Add a manifest

Add a `manifest.json` file beside `package.nix`. Put static web files in a `web/` folder and add a package to `image.packages` that installs them under `usr/lib/teka/app/web/`.

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
| id | Required app ID. The SDK uses it for the image and package. |
| version | Required app release version. |
| display_name | The name people see. |
| nav | The sidebar entry: a label, and a path (always `/app/\<id>`). |
| ui | static shows your web/ files inside TekaOS. external opens a port on the box. |
| backend | none, or proxy to send `/api/v1/app/\<id>/` requests to your app. |
| auth | Set to `teka-proxy-identity` to see who's calling. Learn more in [Box Resources and Firewall](03_box-resources-and-firewall.md). |
| app_cards | Optional status cards. The web interface does not show them. |

> **Note:**
> Bundle your fonts and scripts with the app. Boxes are often offline.

# Using the App API

Every TekaOS box has an HTTP API at http://teka-\<serial>.local/api/v1. Anything you can do on the Apps page, you can do from a script.

## Sign in

Sign in with your box password. You'll get a cookie to send with each request:

```bash
curl -c cookies.txt -X POST -H "Content-Type: application/json" -d '{"password": "<password>"}' http://teka-<serial>.local/api/v1/auth/login
```

## Install an app

Use the `.raw` file in the `result/hello-world` folder that `teka app build` printed. That folder holds one image.

```bash
curl -b cookies.txt -X POST --data-binary @result/hello-world/*.raw "http://teka-<serial>.local/api/v1/manager/install?profile=trusted"
```

You'll get back:

```json
{ "success": true, "app_id": "hello-world" }
```

## Endpoints

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/api/v1/manager` | Lists your apps. |
| POST | `/api/v1/manager/install` | Installs an app. |
| POST | `/api/v1/manager/app/\<id>/start` | Starts an app. |
| POST | `/api/v1/manager/app/\<id>/stop` | Stops an app. |
| PUT | `/api/v1/manager/app/\<id>/boot` | Turns start on boot on or off. |
| POST | `/api/v1/manager/app/\<id>/remove` | Removes an app. |
| GET | `/api/v1/manager/app/\<id>/logs` | Gets an app's log. |

Requests to `/api/v1/app/\<id>/` go straight to your app. If something fails, check [Error Codes](06_error-codes.md).

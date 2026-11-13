# Error Codes

When something goes wrong, the TekaOS API tells you what in this format:

```json
{ "error": { "code": "app_unavailable", "message": "App is not running" } }
```

| Code | HTTP status | What it means |
| --- | --- | --- |
| invalid_request | 400 | Something's wrong with the request. |
| invalid_image | 400 | The file isn't an app image, or it's for a different box type. |
| unauthorized | 401 | Sign in first. |
| forbidden | 403 | You're not allowed to do that. |
| not_found | 404 | That doesn't exist. |
| app_not_found | 404 | No app with that ID is installed. |
| busy | 409 | Something else is in progress. Try again in a moment. |
| internal_error | 500 | Something broke on the box. |
| app_unavailable | 503 | The app isn't running. |

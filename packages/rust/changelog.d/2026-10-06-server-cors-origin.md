**Added**

- **`dirsql server --cors-origin <origin>` lets a browser page on another origin call `/query` and open `/events`.** Every response carries `Access-Control-Allow-Origin: <origin>` (`*` for any origin) and the preflight of a JSON `POST /query` is answered. Without the flag the server still sends no CORS headers. The library gains `ServerConfig::with_cors_origin` and the `ServerConfig::cors_origin` field. (#1399)

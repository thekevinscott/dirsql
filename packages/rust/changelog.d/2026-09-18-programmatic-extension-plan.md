**Added**

`dirsql::extension_resolution::plan_extension_path` plans a single extension
path given outside a config file, the form a constructor's `extensions=[{path}]`
entry takes: path-looking values stay literal (made absolute against a base only
when asked), bare names become a package lookup with a shadow probe. The Python
and TypeScript SDKs each still carry a port of that probe; they migrate onto
this next.

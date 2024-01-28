# ROS Workspace Cleaner

This tool can remove unused packages from an upstream workspace to speed up CI builds.

## Usage:

```
ws_cleaner --upstream upstream_ws --workspace build --action colcon-ignore
```

Multiple workspaces may be specified:

```
ws_cleaner --upstream upstream_ws --workspace build/package_a --workspace build/package_b --action colcon-ignore
```

## Dependency filtering

By default, all dependencies are kept.
The ``--type`` option allows specifying which dependencies should be kept.

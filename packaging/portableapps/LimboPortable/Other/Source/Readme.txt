Limbo Portable
==============

This package is built automatically by the Limbo repositorys CI
(.github/workflows/ci.yml, "Windows host and installer" job):

  LimboPortable.exe     packaging/portableapps/launcher (Rust)
  App\Limbo\Limbo.exe   src-tauri (Rust, Tauri 2, WebView2) + ui (Svelte)
  App\AppInfo\          packaging/portableapps/LimboPortable/App/AppInfo

The launcher starts App\Limbo\Limbo.exe with LIMBO_DATA_DIR set to the
packages Data folder. In that mode Limbo keeps all of its files in Data\
and never writes to the registry or to %LOCALAPPDATA%.

Source: https://github.com/kreisler0/limbo

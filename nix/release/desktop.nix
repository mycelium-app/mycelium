# The desktop client from the release's distributable, run on nixpkgs' JDK instead of the bundled
# runtime, which expects a conventional Linux filesystem.
{
  lib,
  stdenvNoCC,
  jdk21,
  makeWrapper,
  libGL,
  fontconfig,
  freetype,
  xorg,
  glib,
  gtk3,
  gsettings-desktop-schemas,
  version,
  distributable,
}:

stdenvNoCC.mkDerivation {
  pname = "mycelium-desktop";
  inherit version;

  src = distributable;
  nativeBuildInputs = [ makeWrapper ];

  installPhase = ''
    runHook preInstall

    app=$out/share/mycelium-desktop
    mkdir -p $app
    cp -r lib/app/. $app/
    install -Dm644 lib/mycelium.png $out/share/pixmaps/mycelium.png
    install -Dm644 ${./mycelium.desktop} $out/share/applications/mycelium.desktop

    # Skia draws through OpenGL and reads fonts through fontconfig, and GTK's file dialog aborts
    # without the GSettings schemas. None of them are found on NixOS without being named here.
    makeWrapper ${jdk21}/bin/java $out/bin/mycelium-desktop \
      --add-flags "-Djpackage.app-version=${version}" \
      --add-flags "-Dcompose.application.resources.dir=$app/resources" \
      --add-flags "-Dcompose.application.configure.swing.globals=true" \
      --add-flags "-Dskiko.library.path=$app" \
      --add-flags "-cp '$app/*' mycelium.desktop.MainKt" \
      --prefix XDG_DATA_DIRS : ${glib.getSchemaDataDirPath gsettings-desktop-schemas}:${glib.getSchemaDataDirPath gtk3} \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          libGL
          fontconfig
          freetype
          xorg.libX11
          xorg.libXext
          xorg.libXrender
          xorg.libXtst
          xorg.libXi
          xorg.libXrandr
          xorg.libXcursor
        ]
      }

    runHook postInstall
  '';

  meta = {
    description = "Desktop client for Mycelium";
    homepage = "https://github.com/mycelium-app/mycelium";
    license = lib.licenses.wtfpl;
    mainProgram = "mycelium-desktop";
    platforms = [ "x86_64-linux" ];
  };
}

"""Build only the shared OCCT toolkits needed by the owned CAD bridge."""
import os
from pathlib import Path
import platform
import subprocess


def main():
    source = Path('.local/occt-source').resolve()
    build = Path('.local/occt-build').resolve()
    install = Path('.local/occt').resolve()
    args = [
        'cmake', '-S', str(source), '-B', str(build),
        '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_LIBRARY_TYPE=Shared',
        f'-DINSTALL_DIR={install}', f'-DCMAKE_INSTALL_PREFIX={install}',
        '-DINSTALL_DIR_INCLUDE=include/opencascade', '-DINSTALL_DIR_LIB=lib',
        '-DINSTALL_DIR_BIN=bin', '-DINSTALL_DIR_RESOURCE=share/occt/resources',
        '-DBUILD_ADDITIONAL_TOOLKITS=TKDESTEP;TKDEIGES;TKMesh',
        '-DBUILD_USE_VCPKG=OFF', '-DBUILD_DOC_Overview=OFF',
        '-DBUILD_SAMPLES_QT=OFF', '-DBUILD_SAMPLES_MFC=OFF',
        '-DINSTALL_SAMPLES=OFF', '-DINSTALL_TEST_CASES=OFF',
    ]
    # OCCT automatically adds the transitive dependencies of the selected toolkits.
    for line in (source / 'adm/MODULES').read_text().splitlines():
        if line.strip():
            args.append(f'-DBUILD_MODULE_{line.split()[0]}=OFF')
    for option in ('TK', 'FREETYPE', 'FREEIMAGE', 'FFMPEG', 'OPENVR', 'RAPIDJSON',
                   'DRACO', 'TBB', 'EIGEN', 'XLIB', 'OPENGL', 'GLES2', 'D3D'):
        args.append(f'-DUSE_{option}=OFF')
    system = platform.system()
    if system == 'Windows':
        args += ['-G', 'Visual Studio 17 2022', '-A', 'x64']
    elif system == 'Darwin':
        args += ['-DCMAKE_OSX_ARCHITECTURES=arm64', '-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0',
                 '-DINSTALL_NAME_DIR=@rpath', '-DCMAKE_INSTALL_RPATH=@loader_path']
    else:
        args += ['-DCMAKE_INSTALL_RPATH=$ORIGIN']
    subprocess.run(args, check=True)
    # Bound memory use when building the large C++ translation units.
    subprocess.run(['cmake', '--build', str(build), '--config', 'Release',
                    '--parallel', '2'], check=True)
    subprocess.run(['cmake', '--install', str(build), '--config', 'Release'], check=True)
    with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as output:
        output.write(f'OCCT_ROOT={install}\n')
        if system == 'Linux':
            output.write(f'LD_LIBRARY_PATH={install / "lib"}\n')
        elif system == 'Darwin':
            output.write(f'DYLD_LIBRARY_PATH={install / "lib"}\n')
    if system == 'Windows':
        with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as output:
            output.write(f'{install / "bin"}\n')


if __name__ == '__main__':
    main()

"""Stage and verify relocatable OCCT runtimes for the desktop release packages."""
import argparse
import hashlib
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import xml.etree.ElementTree as ET

SDK = Path(os.environ.get('OCCT_ROOT', '.local/occt')).resolve()
SOURCE = Path(os.environ.get('OCCT_SOURCE', '.local/occt-source')).resolve()
BUILD = Path(os.environ.get('OCCT_BUILD', '.local/occt-build')).resolve()
NOTICE = '''Open CAM Studio uses Open CASCADE Technology (OCCT) 7.9.3.
Copyright (c) Open CASCADE SAS and other contributors.
OCCT is distributed under LGPL 2.1 with the Open CASCADE exception.
The accompanying LICENSE_LGPL_21.txt and OCCT_LGPL_EXCEPTION.txt apply to OCCT.
The corresponding source is available as OpenCAMStudio-OCCT-7.9.3-source.tar.gz
in official releases, built from:
https://github.com/Open-Cascade-SAS/OCCT/commit/a016080bf6738d6aeae020badee4e888ad1540a5
Build instructions are in OpenCAMStudio's .github/scripts/build-occt.py.
The shared libraries can be replaced by a compatible OCCT build.
Windows packages also contain Microsoft's Visual C++ runtime redistributables.
'''


def copy_matching(source, pattern, destination):
    files = sorted(source.glob(pattern))
    if not files:
        raise RuntimeError(f'No runtime files matching {source / pattern}')
    destination.mkdir(parents=True, exist_ok=True)
    for item in files:
        shutil.copy2(item, destination / item.name)


def copy_darwin_dependencies(executable, sdk_lib, destination):
    """Copy the non-system dylib dependency closure used by the app."""
    available = {path.name: path for path in sdk_lib.glob('*.dylib')}
    pending = [executable]
    copied = set()
    destination.mkdir(parents=True, exist_ok=True)
    while pending:
        image = pending.pop()
        if image in copied:
            continue
        copied.add(image)
        output = subprocess.check_output(['otool', '-L', str(image)], text=True)
        for line in output.splitlines()[1:]:
            dependency = line.strip().split(' (compatibility')[0]
            if dependency.startswith(('/System/Library/', '/usr/lib/')):
                continue
            name = Path(dependency).name
            library = available.get(name)
            if library is None:
                raise RuntimeError(f'Cannot bundle {dependency}, required by {image}')
            bundled = destination / name
            if name not in {item.name for item in destination.iterdir()}:
                shutil.copy2(library, bundled)
            pending.append(bundled)


def stage(destination, system):
    if system == 'Linux':
        libraries = destination / 'usr/lib'
        resources = destination / 'usr/share/opencamstudio/occt'
        licenses = destination / 'usr/share/licenses/opencamstudio'
        copy_matching(SDK / 'lib', 'libTK*.so*', libraries)
    elif system == 'Darwin':
        libraries = destination / 'Contents/Frameworks'
        resources = destination / 'Contents/Resources/occt'
        licenses = destination / 'Contents/Resources/licenses'
        if not any((SDK / 'lib').glob('*.dylib')):
            raise RuntimeError(f'No runtime files matching {SDK / "lib/*.dylib"}')
        for name in ('opencamstudio', 'opencamstudio-mcp'):
            executable = destination / 'Contents/MacOS' / name
            if not executable.is_file():
                if name == 'opencamstudio':
                    raise FileNotFoundError(executable)
                continue
            copy_darwin_dependencies(executable, SDK / 'lib', libraries)
            subprocess.run(['install_name_tool', '-add_rpath', '@executable_path/../Frameworks',
                            str(executable)], check=True)
    else:
        libraries = destination
        resources = destination / 'occt-resources'
        licenses = destination / 'licenses'
        copy_matching(SDK / 'bin', 'TK*.dll', libraries)
        redist = Path(os.environ['VCToolsRedistDir']) / 'x64/Microsoft.VC143.CRT'
        copy_matching(redist, '*.dll', libraries)
    resource_source = SDK / 'share/occt/resources'
    if not resource_source.is_dir():
        resource_source = SDK / 'share/opencascade/resources'
    if not resource_source.is_dir():
        raise FileNotFoundError(f'Missing OCCT resources below {SDK / "share"}')
    shutil.copytree(resource_source, resources, dirs_exist_ok=True)
    licenses.mkdir(parents=True, exist_ok=True)
    license_source = SOURCE if SOURCE.is_dir() else SDK / 'share/doc/opencascade'
    for name in ('LICENSE_LGPL_21.txt', 'OCCT_LGPL_EXCEPTION.txt'):
        path = license_source / name
        if not path.is_file():
            raise FileNotFoundError(f'Missing OCCT license {path}')
        shutil.copy2(path, licenses / name)
    shutil.copy2('LICENSE', licenses / 'OpenCAMStudio-GPL-3.0.txt')
    (licenses / 'OCCT-NOTICE.txt').write_text(NOTICE, encoding='utf-8')


def wix_fragment(stage_dir, output):
    """Include the same dependency tree in the MSI as in the portable ZIP."""
    namespace = 'http://schemas.microsoft.com/wix/2006/wi'
    ET.register_namespace('', namespace)
    def node(parent, tag, **attributes):
        return ET.SubElement(parent, f'{{{namespace}}}{tag}', attributes)
    root = ET.Element(f'{{{namespace}}}Wix')
    fragment = node(root, 'Fragment')
    directory = node(fragment, 'DirectoryRef', Id='Bin')
    feature = node(fragment, 'FeatureRef', Id='Binaries')
    directories = {Path('.'): directory}
    for file in sorted(stage_dir.rglob('*')):
        relative = file.relative_to(stage_dir)
        identifier = hashlib.sha256(relative.as_posix().encode()).hexdigest()[:24]
        if file.is_dir():
            directories[relative] = node(directories[relative.parent], 'Directory',
                                         Id=f'OcctDir{identifier}', Name=file.name)
        elif relative != Path('opencamstudio.exe'):
            component = node(directories[relative.parent], 'Component',
                             Id=f'OcctComponent{identifier}', Guid='*', Win64='yes')
            node(component, 'File', Id=f'OcctFile{identifier}', Source=str(file.resolve()),
                 Name=file.name, KeyPath='yes')
            node(feature, 'ComponentRef', Id=f'OcctComponent{identifier}')
    ET.ElementTree(root).write(output, encoding='utf-8', xml_declaration=True)


def verify(destination, smoke, system):
    """Import real fixtures with neither the SDK nor its source tree available."""
    executable_dir = destination
    if system == 'Linux':
        executable_dir /= 'usr/bin'
    elif system == 'Darwin':
        executable_dir /= 'Contents/MacOS'
    app_executable = executable_dir / ('opencamstudio.exe' if system == 'Windows'
                                       else 'opencamstudio')
    probe = executable_dir / smoke.name
    shutil.copy2(smoke, probe)
    if system == 'Darwin':
        subprocess.run(['install_name_tool', '-add_rpath', '@executable_path/../Frameworks',
                        str(probe)], check=True)
        # install_name_tool invalidates arm64's linker-generated ad-hoc signature.
        for binary in [*executable_dir.iterdir(), *(destination / 'Contents/Frameworks').glob('*.dylib')]:
            subprocess.run(['codesign', '--force', '--sign', '-', str(binary)], check=True)
    env = {key: value for key, value in os.environ.items()
           if key not in ('LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH', 'OCCT_ROOT',
                          'OCCT_SOURCE', 'OCCT_BUILD', 'OCCT_INCLUDE_DIR',
                          'OCCT_LIB_DIR') and not key.startswith('CSF_')}
    env['PATH'] = os.pathsep.join(p for p in env.get('PATH', '').split(os.pathsep)
                                  if str(SDK).lower() not in p.lower())
    if system == 'Linux':
        env['LD_LIBRARY_PATH'] = str(destination / 'usr/lib')
    fixtures = Path('crates/cam-kernel/fixtures').resolve()
    hidden = []
    try:
        for path in (SDK, SOURCE, BUILD):
            if not path.exists():
                continue
            renamed = path.with_name(path.name + '-unavailable')
            path.rename(renamed)
            hidden.append((path, renamed))
        subprocess.run([str(probe), str(fixtures / 'box-mm.step'),
                        str(fixtures / 'box-mm.iges')], env=env, cwd=destination, check=True)
        # Also start the packaged desktop executable without the SDK or loader-path
        # environment. --help exits before opening a window, but loads the GUI binary
        # and proves its complete OCCT runtime dependency chain is relocatable.
        subprocess.run([str(app_executable), '--help'], env=env,
                       cwd=destination, check=True)
        mcp_executable = executable_dir / ('opencamstudio-mcp.exe' if system == 'Windows'
                                           else 'opencamstudio-mcp')
        if mcp_executable.is_file():
            subprocess.run([str(mcp_executable), '--help'], env=env,
                           cwd=destination, check=True)
    finally:
        for original, renamed in reversed(hidden):
            renamed.rename(original)
        probe.unlink(missing_ok=True)


def source_archive(output):
    with tarfile.open(output, 'w:gz') as archive:
        def exclude_git(info):
            return None if '.git' in Path(info.name).parts else info
        archive.add(SOURCE, arcname='OCCT-7.9.3', filter=exclude_git)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('stage', 'verify', 'wix', 'source'))
    parser.add_argument('destination', type=Path)
    parser.add_argument('--smoke', type=Path)
    args = parser.parse_args()
    destination = args.destination.resolve()
    if args.operation == 'stage':
        stage(destination, platform.system())
    elif args.operation == 'verify':
        if args.smoke is None:
            parser.error('verify requires --smoke')
        verify(destination, args.smoke.resolve(), platform.system())
    elif args.operation == 'wix':
        wix_fragment(destination, Path('target/occt-runtime.wxs'))
    else:
        source_archive(destination)


if __name__ == '__main__':
    main()

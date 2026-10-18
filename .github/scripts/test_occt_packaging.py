"""Checks for runtime staging and installer manifests; run with unittest discover."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('package_occt', SCRIPTS / 'package-occt.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        self.sdk = self.root / '.local/occt'
        self.source = self.root / '.local/occt-source'
        for path in (self.sdk / 'lib', self.sdk / 'bin',
                     self.sdk / 'share/occt/resources/StdResource', self.source,
                     self.root / '.local/occt-build'):
            path.mkdir(parents=True, exist_ok=True)
        for name in ('LICENSE_LGPL_21.txt', 'OCCT_LGPL_EXCEPTION.txt'):
            (self.source / name).write_text(name)
        (self.sdk / 'share/occt/resources/StdResource/Plugin').write_text('resource')
        Path('LICENSE').write_text('application license')
        self.patcher = patch.multiple(package, SDK=self.sdk, SOURCE=self.source)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def test_windows_staging_and_msi_manifest_include_every_dependency(self):
        (self.sdk / 'bin/TKernel.dll').write_bytes(b'OCCT')
        redist = self.root / 'VC Redist/x64/Microsoft.VC143.CRT'
        redist.mkdir(parents=True)
        (redist / 'vcruntime140.dll').write_bytes(b'CRT')
        destination = self.root / 'portable app'
        with patch.dict(os.environ, VCToolsRedistDir=str(redist.parent.parent)):
            package.stage(destination, 'Windows')
        (destination / 'opencamstudio.exe').write_bytes(b'application')
        fragment = self.root / 'runtime.wxs'
        package.wix_fragment(destination, fragment)
        xml = ET.parse(fragment)
        ns = {'w': 'http://schemas.microsoft.com/wix/2006/wi'}
        files = xml.findall('.//w:File', ns)
        expected = {p.resolve() for p in destination.rglob('*')
                    if p.is_file() and p.name != 'opencamstudio.exe'}
        self.assertEqual({Path(f.attrib['Source']) for f in files}, expected)
        self.assertTrue((destination / 'TKernel.dll').is_file())
        self.assertTrue((destination / 'vcruntime140.dll').is_file())
        self.assertTrue((destination / 'occt-resources/StdResource/Plugin').is_file())
        self.assertTrue((destination / 'licenses/OCCT-NOTICE.txt').is_file())
        components = xml.findall('.//w:Component', ns)
        references = xml.findall('.//w:ComponentRef', ns)
        self.assertEqual({c.attrib['Id'] for c in components}, {r.attrib['Id'] for r in references})
        self.assertEqual(len(components), len(expected))
        # Component IDs remain stable across rebuilds of the same installed paths.
        previous = fragment.read_bytes()
        package.wix_fragment(destination, fragment)
        self.assertEqual(previous, fragment.read_bytes())

    def test_missing_runtime_fails(self):
        with self.assertRaisesRegex(RuntimeError, 'No runtime files'):
            package.stage(self.root / 'empty', 'Darwin')

    def test_corresponding_source_contains_license_and_excludes_git(self):
        (self.source / '.git').mkdir()
        (self.source / '.git/config').write_text('private checkout metadata')
        (self.source / 'CMakeLists.txt').write_text('source')
        output = self.root / 'source.tar.gz'
        package.source_archive(output)
        with tarfile.open(output) as archive:
            names = archive.getnames()
        self.assertIn('OCCT-7.9.3/LICENSE_LGPL_21.txt', names)
        self.assertIn('OCCT-7.9.3/CMakeLists.txt', names)
        self.assertFalse(any('.git' in Path(n).parts for n in names))

    @unittest.skipUnless(shutil.which('g++') and os.name == 'posix' and
                         os.uname().sysname == 'Linux', 'Linux native loader check')
    def test_relocated_linux_dependency_chain_and_cleanup_after_failure(self):
        # A real shared-library chain tests loader behavior with the SDK hidden.
        Path('kernel.cpp').write_text('extern "C" int kernel() { return 42; }')
        Path('math.cpp').write_text('extern "C" int kernel(); extern "C" int math() { return kernel(); }')
        Path('probe.cpp').write_text('extern "C" int math(); int main() { return math() != 42; }')
        library_dir = str(self.sdk / 'lib')
        subprocess.run(['g++', '-shared', '-fPIC', 'kernel.cpp', '-o',
                        str(self.sdk / 'lib/libTKernel.so')], check=True)
        subprocess.run(['g++', '-shared', '-fPIC', 'math.cpp', '-L', library_dir,
                        '-lTKernel', '-Wl,-rpath,$ORIGIN', '-o',
                        str(self.sdk / 'lib/libTKMath.so')], check=True)
        smoke = self.root / 'probe'
        subprocess.run(['g++', 'probe.cpp', '-L', library_dir, '-lTKMath',
                        f'-Wl,-rpath,{library_dir}', '-o', str(smoke)], check=True)
        destination = self.root / 'extracted AppImage'
        (destination / 'usr/bin').mkdir(parents=True)
        package.stage(destination, 'Linux')
        package.verify(destination, smoke, 'Linux')
        self.assertTrue(self.sdk.is_dir())
        self.assertTrue(self.source.is_dir())
        self.assertFalse((destination / 'usr/bin/probe').exists())
        # A missing transitive dependency must fail and still restore the SDK.
        (destination / 'usr/lib/libTKernel.so').unlink()
        with self.assertRaises(subprocess.CalledProcessError):
            package.verify(destination, smoke, 'Linux')
        self.assertTrue(self.sdk.is_dir())
        self.assertTrue(self.source.is_dir())
        self.assertFalse((destination / 'usr/bin/probe').exists())


if __name__ == '__main__':
    unittest.main()

// Pinned, private static dependencies keep the distributed native closure to
// the existing FFmpeg shared libraries. No host codec packages are discovered.
import { execFileSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync, copyFileSync } from 'node:fs';
import { join, resolve, delimiter, win32 } from 'node:path';
const root = resolve(import.meta.dirname, '..');
const manifest = JSON.parse(readFileSync(join(root, 'hifimule-daemon/audio-runtime.json')));
export const encodingDependencyReceipt = manifest.encodingDependencies;
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
export function windowsTarExecutable(env = process.env) {
  const systemRoot = Object.entries(env).find(([key]) => key.toUpperCase() === 'SYSTEMROOT')?.[1] ?? 'C:\\Windows';
  return win32.join(systemRoot, 'System32', 'tar.exe');
}
export function buildEncodingDependencies({ cache, work, prefix, target, env = process.env, execute = execFileSync, windows = false }) {
  env = { ...env, PKG_CONFIG_PATH: [join(prefix, 'lib/pkgconfig'), env.PKG_CONFIG_PATH].filter(Boolean).join(delimiter) };
  const run = (cmd, args, cwd) => execute(cmd, args, { cwd, env, stdio: 'inherit' });
  const args = ['-DBUILD_SHARED_LIBS=OFF', '-DCMAKE_POSITION_INDEPENDENT_CODE=ON', '-DCMAKE_BUILD_TYPE=Release', `-DCMAKE_INSTALL_PREFIX=${prefix}`, '-DCMAKE_INSTALL_LIBDIR=lib', `-DCMAKE_PREFIX_PATH=${prefix}`, '-DBUILD_TESTING=OFF', '-DCMAKE_POLICY_VERSION_MINIMUM=3.5'];
  if (target.includes('apple')) args.push(`-DCMAKE_OSX_ARCHITECTURES=${target.startsWith('aarch64') ? 'arm64' : 'x86_64'}`, `-DCMAKE_OSX_DEPLOYMENT_TARGET=${target.startsWith('aarch64') ? '11.0' : '10.15'}`);
  if (windows) args.push('-G', 'NMake Makefiles', '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL');
  if (windows) {
    try { run('cmake', ['--version']); } catch (error) { throw new Error('CMake is required for the pinned Windows encoding dependencies; install CMake and use a Visual Studio Native Tools environment', { cause: error }); }
  }
  mkdirSync(cache, { recursive: true });
  for (const dep of manifest.encodingDependencies) {
    const archive = join(cache, dep.archive);
    if (!existsSync(archive) || hash(archive) !== dep.sha256) {
      const temporary = `${archive}.tmp-${randomUUID()}`;
      try {
        // File output lets curl discard partial transfers before retrying TLS/network errors.
        run(windows ? 'curl.exe' : 'curl', ['--fail', '--location', '--retry', '5', '--retry-all-errors', '--connect-timeout', '30', '--max-time', '300', '--retry-max-time', '600', '--output', temporary, dep.url]);
        if (hash(temporary) !== dep.sha256) throw new Error(`${dep.name} source checksum mismatch`);
        renameSync(temporary, archive);
      } finally { rmSync(temporary, { force: true }); }
    }
    // MSYS2 tar on PATH rejects native Windows archive and destination paths.
    // Use the OS-provided BSD tar, which accepts both, regardless of PATH order.
    run(windows ? windowsTarExecutable(env) : 'tar', ['-xf', dep.archive, '-C', resolve(work)], cache);
    const source = join(work, dep.directory);
    if (!windows) {
      if (target.includes('apple')) {
        // Older upstream libtool emits a linker flag removed by modern Apple ld.
        for (const name of ['configure', 'ltmain.sh']) { const path = join(source, name); if (existsSync(path)) writeFileSync(path, readFileSync(path, 'utf8').replaceAll('-force_cpusubtype_ALL', '')); }
      }
      const options = dep.name === 'lame' ? ['--disable-frontend', '--disable-decoder', '--disable-nasm'] : dep.name === 'opus' ? ['--disable-extra-programs', '--disable-doc'] : [];
      run('./configure', [`--prefix=${prefix}`, '--disable-shared', '--enable-static', '--with-pic', ...options], source);
      run('make', ['-j4'], source); run('make', ['install'], source);
    } else {
      if (dep.name === 'lame') {
        // LAME ships an MSVC config but no CMake project. Build only its library,
        // with upstream source files and no decoder/front-end/assembly.
        copyFileSync(join(source, 'configMS.h'), join(source, 'config.h'));
        writeFileSync(join(source, 'CMakeLists.txt'), `cmake_minimum_required(VERSION 3.15)\nproject(lame C)\nfile(GLOB sources libmp3lame/*.c)\nadd_library(mp3lame STATIC \${sources} libmp3lame/vector/xmm_quantize_sub.c)\ntarget_compile_definitions(mp3lame PRIVATE HAVE_CONFIG_H _CRT_SECURE_NO_WARNINGS)\ntarget_include_directories(mp3lame PRIVATE . include libmp3lame)\ninstall(TARGETS mp3lame ARCHIVE DESTINATION lib)\ninstall(FILES include/lame.h DESTINATION include/lame)\n`);
      }
      const build = join(source, 'build-hifimule');
      const options = dep.name === 'opus' ? ['-DOPUS_BUILD_PROGRAMS=OFF', '-DOPUS_BUILD_TESTING=OFF', '-DOPUS_INSTALL_PKG_CONFIG_MODULE=ON'] : [];
      run('cmake', ['-S', source, '-B', build, ...args, ...options]);
      run('cmake', ['--build', build, '--config', 'Release', '--parallel', '4']);
      run('cmake', ['--install', build, '--config', 'Release']);
      // Explicit private .pc metadata avoids upstream platform-dependent install
      // defaults. FFmpeg's pkgconf runs in MSYS2 and links MSVC static libraries.
      const packages = dep.name === 'vorbis' ? [['vorbis', 'ogg', '-lvorbis'], ['vorbisenc', 'vorbis', '-lvorbisenc']] : [[dep.name === 'lame' ? 'lame' : dep.name, '', dep.name === 'lame' ? '-lmp3lame' : `-l${dep.name}`]];
      const pkgdir = join(prefix, 'lib/pkgconfig'); mkdirSync(pkgdir, { recursive: true });
      const portablePrefix = prefix.replaceAll('\\', '/').replace(/^([A-Za-z]):/, (_, drive) => `/${drive.toLowerCase()}`);
      // Opus consumers include bare opus*.h names; upstream installs them in
      // include/opus and advertises that subdirectory in its pkg-config flags.
      const includeSuffix = dep.name === 'opus' ? '/opus' : '';
      for (const [name, requires, libs] of packages) writeFileSync(join(pkgdir, `${name}.pc`), `prefix=${portablePrefix}\nlibdir=\${prefix}/lib\nincludedir=\${prefix}/include\nName: ${name}\nDescription: HifiMule private ${name}\nVersion: ${dep.version}\nRequires.private: ${requires}\nLibs: -L\${libdir} ${libs}\nCflags: -I\${includedir}${includeSuffix}\n`);

    }
    const notices = join(prefix, 'share/licenses', dep.name); mkdirSync(notices, { recursive: true });
    const license = existsSync(join(source, 'COPYING')) ? 'COPYING' : 'LICENSE';
    copyFileSync(join(source, license), join(notices, license));
  }
  return { ...env, PKG_CONFIG_PATH: [join(prefix, 'lib/pkgconfig'), env.PKG_CONFIG_PATH].filter(Boolean).join(delimiter) };
}

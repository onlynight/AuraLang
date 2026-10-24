#!/bin/bash
# P398 影子读探针跑批：photon 编译 → 运行 → 与 seed 参考对比
cd /d/Code/AuraLang
for p in build/p398probes/probe*.aura; do
    name=$(basename "$p" .aura)
    # seed 参考（rust VM，即正确性金标准）
    expect=$(./seed/target/release/aura.exe run "$p" 2>/dev/null | grep "^p")
    cp "$p" build/hat-vmhost/current.aura
    AURA_MEM_LIMIT_MB=12288 ./build/hat-native/PhotonHatCompile.exe > /tmp/p398_compile.log 2>&1
    if ! grep -q "===RESULT===success" /tmp/p398_compile.log; then
        echo "$name: COMPILE-FAIL"; continue
    fi
    MSYS2_ARG_CONV_EXCL="*" "D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe" build/hat-vmhost/current.obj build/hat-vmhost/aura_runtime.obj /OUT:build/hat-vmhost/current.exe "C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64/kernel32.lib" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB /STACK:16777216 > /dev/null 2>&1
    got=$(timeout 20 ./build/hat-vmhost/current.exe 2>/dev/null | grep "^p" | tail -1)
    if [ "$expect" == "$got" ]; then
        echo "$name: PASS [$got]"
    else
        echo "$name: FAIL expect=[$expect] got=[$got]"
    fi
done

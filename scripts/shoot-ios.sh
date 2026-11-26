#!/bin/sh
# 사용: shoot-ios.sh <시뮬레이터 id> <SideDesk.app 경로> <저장 폴더>
# 디버그 빌드의 보기용 값(-demo)으로 화면을 찍는다. 연결, 블루투스, 위치 권한은 쓰지 않는다.
set -eu
sim=$1
app=$2
out=$3
bundle=io.github.hyeonsikoh.sidedesk
mkdir -p "$out"
xcrun simctl bootstatus "$sim" -b > /dev/null
xcrun simctl install "$sim" "$app"

shot() {
    name=$1
    shift
    xcrun simctl terminate "$sim" "$bundle" 2> /dev/null || true
    xcrun simctl launch "$sim" "$bundle" -demo YES "$@" > /dev/null
    sleep 3
    xcrun simctl io "$sim" screenshot --type=png "$out/$name.png" > /dev/null
    echo "$out/$name.png"
}

for theme in Dark Light; do
    lower=$(echo "$theme" | tr 'A-Z' 'a-z')
    for orient in portrait landscape; do
        landscape=NO
        [ "$orient" = landscape ] && landscape=YES
        shot "home-$orient-$lower" -theme "$theme" -face Digits -landscape "$landscape"
        shot "perf-$orient-$lower" -theme "$theme" -face Digits -screen perf -landscape "$landscape"
        shot "processes-$orient-$lower" -theme "$theme" -face Digits -screen processes -landscape "$landscape"
    done
done
for face in Dial Weather Calendar Overview; do
    shot "face-$(echo "$face" | tr 'A-Z' 'a-z')" -theme Dark -face "$face"
done
shot picking -theme Dark -face Digits -picking YES
shot kill-confirm -theme Dark -face Digits -screen processes -ask YES
shot location-notice -theme Dark -face Digits -locationNotice YES
shot pairing -theme Dark -pairing 123456
shot lost -theme Dark -face Digits -lost YES
xcrun simctl terminate "$sim" "$bundle" 2> /dev/null || true

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
# 상태 줄을 늘 같게 둔다.
xcrun simctl status_bar "$sim" override --time 9:41 --batteryState charged --batteryLevel 100 \
    --wifiBars 3 --cellularMode notSupported > /dev/null 2>&1 || true
# 부팅 직후 뜨는 시스템 알림(Apple Intelligence 안내 등)이 사라지도록 한 번 띄워 두고 기다린 뒤 버린다.
xcrun simctl launch "$sim" "$bundle" -demo YES > /dev/null
sleep 25

shot() {
    name=$1
    shift
    xcrun simctl terminate "$sim" "$bundle" 2> /dev/null || true
    xcrun simctl launch "$sim" "$bundle" -demo YES "$@" > /dev/null
    sleep 3
    xcrun simctl io "$sim" screenshot --type=png "$out/$name.png" > /dev/null
    # 화면 찍기는 기기 세로 방향으로 나오므로, 가로로 그린 화면은 돌려 둔다.
    case "$name" in
        *landscape*) sips -r 270 "$out/$name.png" > /dev/null ;;
    esac
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
        shot "ai-$orient-$lower" -theme "$theme" -face Digits -screen ai -landscape "$landscape"
        shot "shortcuts-$orient-$lower" -theme "$theme" -face Digits -screen shortcuts -landscape "$landscape"
        shot "music-$orient-$lower" -theme "$theme" -face Digits -screen music -landscape "$landscape"
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
shot ai-alert -theme Dark -face Digits -alert YES
shot rest -theme Dark -face Digits -rest YES
shot rest-panel -theme Dark -face Digits -restPanel YES
xcrun simctl terminate "$sim" "$bundle" 2> /dev/null || true

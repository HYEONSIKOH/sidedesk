# 배포 빌드에서 빌드한 사람의 홈 경로를 지우는 조각. source 해서 쓴다.
# 컴파일러는 실행 파일에 경로(패닉 위치, 의존 크레이트)를 넣는다. 이것을 짧은 이름으로 바꾼다.
#
# sidedesk_remap <저장소>  → SIDEDESK_REMAP(rustc 인자), SIDEDESK_CMAP(C 컴파일러 인자)
# RUSTFLAGS 는 내보내지 않는다. cargo 는 RUSTFLAGS 가 있으면 CARGO_TARGET_<대상>_RUSTFLAGS 를
# 아무 말 없이 버리므로(안드로이드 16KB 정렬이 빠짐) 부르는 쪽이 알맞은 변수에 붙인다.
sidedesk_remap() {
    root=$1
    # RUSTFLAGS 는 빈칸으로 나뉘므로 경로에 빈칸이 있으면 멈춘다.
    case "$root$HOME" in
        *" "*)
            echo "저장소나 홈 경로에 빈칸이 있어 경로 바꾸기를 할 수 없습니다" >&2
            return 1
            ;;
    esac
    # 여럿이 맞으면 뒤에 준 것이 이기므로 넓은 홈 경로를 맨 앞에 둔다.
    pairs="$HOME=home
$HOME/.rustup=rustup
$HOME/.cargo/git/checkouts=git
$HOME/.cargo/registry/src=crates
$root=sidedesk"
    SIDEDESK_REMAP=
    SIDEDESK_CMAP=
    for pair in $pairs; do
        SIDEDESK_REMAP="$SIDEDESK_REMAP --remap-path-prefix=$pair"
        SIDEDESK_CMAP="$SIDEDESK_CMAP -ffile-prefix-map=$pair"
    done
    export SIDEDESK_REMAP SIDEDESK_CMAP
}

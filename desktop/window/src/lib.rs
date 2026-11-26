//! 설정 창. HTML 은 실행 파일 안에 있고, 창 안의 웹 화면으로 띄운다. 포트를 열지 않는다.

mod board;
mod json;
mod view;

pub use board::{Board, Computer};
pub use view::open;

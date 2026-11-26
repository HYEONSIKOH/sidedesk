//! 앨범 그림 줄이기. 운영체제의 WIC 로 풀고, 줄이고, JPEG 로 다시 담는다(맥 image.rs 와 같은 크기와 상한).

use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::ManuallyDrop;

use sidedesk_domain::{ART_LIMIT, BLE_ART_LIMIT, Picture};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    IWICBitmapFrameEncode, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{
    CreateStreamOnHGlobal, IPropertyBag2, PROPBAG2,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, IStream, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_R4};
use windows::core::PWSTR;

use crate::com::mta;
use crate::handle::wide;

/// 받아 줄 원본 크기 상한(맥과 같음)
pub const SOURCE_LIMIT: usize = 10 * 1024 * 1024;
const SIDE: u32 = 320;
const SMALL_SIDE: u32 = 240;
/// 이보다 큰 그림은 풀지 않는다(작은 파일이 큰 그림으로 풀려 메모리를 많이 쓰는 것을 막음).
const MAX_PIXELS_SIDE: u32 = 8192;
const QUALITIES: [f32; 3] = [0.8, 0.6, 0.4];

/// 연결 상한에 맞는 JPEG. 블루투스 상한을 넘으면 작은 판도 만든다. 못 풀면 없음.
pub fn shrink(source: &[u8]) -> Option<Picture> {
    let mut picture = shrink_to(source, SIDE, ART_LIMIT)?;
    if picture.jpeg.len() > BLE_ART_LIMIT {
        picture.small = shrink_to(source, SMALL_SIDE, BLE_ART_LIMIT).map(Box::new);
    }
    Some(picture)
}

/// 긴 변이 `side` 를 넘지 않게 맞춘 크기
fn fit(width: u32, height: u32, side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= side {
        return (width.max(1), height.max(1));
    }
    let scale = |n: u32| ((u64::from(n) * u64::from(side)) / u64::from(long)).max(1) as u32;
    (scale(width), scale(height))
}

fn shrink_to(source: &[u8], side: u32, limit: usize) -> Option<Picture> {
    if source.is_empty() || source.len() > SOURCE_LIMIT {
        return None;
    }
    mta();
    // SAFETY: MTA 안에서 WIC 객체를 만들고, source 는 이 함수가 끝날 때까지 산다.
    let jpeg = unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let input = factory.CreateStream().ok()?;
        input.InitializeFromMemory(source).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&input, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || width.max(height) > MAX_PIXELS_SIDE {
            return None;
        }
        let (w, h) = fit(width, height, side);
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, w, h, WICBitmapInterpolationModeFant)
            .ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        QUALITIES
            .iter()
            .filter_map(|quality| encode(&factory, &converter, w, h, *quality))
            .find(|bytes| bytes.len() <= limit)
    }?;
    let mut hasher = DefaultHasher::new();
    jpeg.hash(&mut hasher);
    Some(Picture {
        hash: hasher.finish().max(1),
        jpeg,
        small: None,
    })
}

/// # Safety
/// MTA 안에서, 살아 있는 WIC 객체로 부른다.
unsafe fn encode(
    factory: &IWICImagingFactory,
    source: &windows::Win32::Graphics::Imaging::IWICFormatConverter,
    width: u32,
    height: u32,
    quality: f32,
) -> Option<Vec<u8>> {
    // SAFETY: 함수 약속대로. 메모리 스트림은 놓을 때 함께 풀린다.
    unsafe {
        let stream: IStream = CreateStreamOnHGlobal(Default::default(), true).ok()?;
        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())
            .ok()?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut options: Option<IPropertyBag2> = None;
        encoder.CreateNewFrame(&mut frame, &mut options).ok()?;
        let (frame, options) = (frame?, options?);
        let mut name = wide("ImageQuality");
        let bag = PROPBAG2 {
            pstrName: PWSTR(name.as_mut_ptr()),
            ..Default::default()
        };
        let value = VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VT_R4,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 { fltVal: quality },
                }),
            },
        };
        options.Write(1, &bag, &value).ok()?;
        frame.Initialize(&options).ok()?;
        frame.SetSize(width, height).ok()?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format).ok()?;
        frame.WriteSource(source, std::ptr::null()).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
        read_all(&stream)
    }
}

/// # Safety
/// 살아 있는 스트림
unsafe fn read_all(stream: &IStream) -> Option<Vec<u8>> {
    // SAFETY: 함수 약속대로. 길이만큼만 읽는다.
    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let length = usize::try_from(stat.cbSize).ok()?;
        if length > ART_LIMIT * 4 {
            return None;
        }
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut bytes = vec![0u8; length];
        let mut read = 0u32;
        stream
            .Read(bytes.as_mut_ptr().cast(), length as u32, Some(&mut read))
            .ok()
            .ok()?;
        bytes.truncate(read as usize);
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 무늬가 있는 24비트 BMP(압축 없음)
    fn bmp(width: u32, height: u32) -> Vec<u8> {
        let row = (width * 3).div_ceil(4) * 4;
        let pixels = row * height;
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(54 + pixels).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&pixels.to_le_bytes());
        out.extend_from_slice(&[0; 16]);
        let mut seed = 7u32;
        for y in 0..height {
            for x in 0..width {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let noise = (seed >> 16) as u8;
                out.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, noise]);
            }
            out.extend(std::iter::repeat_n(0, (row - width * 3) as usize));
        }
        out
    }

    #[test]
    fn long_side_fits() {
        assert_eq!(fit(1000, 500, 320), (320, 160));
        assert_eq!(fit(500, 1000, 240), (120, 240));
        assert_eq!(fit(100, 50, 320), (100, 50));
        assert_eq!(fit(10000, 1, 320), (320, 1));
    }

    #[test]
    #[ignore = "WIC 를 부른다. CI 윈도우 작업에서 돌린다"]
    fn windows_only_shrinks_to_jpeg_under_the_limits() {
        let picture = shrink(&bmp(1200, 900)).expect("줄인 그림");
        eprintln!(
            "큰 판 {}바이트, 작은 판 {:?}",
            picture.jpeg.len(),
            picture.small.as_ref().map(|s| s.jpeg.len())
        );
        assert!(picture.jpeg.starts_with(&[0xFF, 0xD8]));
        assert!(picture.jpeg.len() <= ART_LIMIT);
        if picture.jpeg.len() > BLE_ART_LIMIT {
            let small = picture.small.as_ref().expect("작은 판");
            assert!(small.jpeg.len() <= BLE_ART_LIMIT);
            assert_ne!(small.hash, picture.hash);
        }
        assert!(shrink(b"not an image").is_none());
        assert!(shrink(&[]).is_none());
        // 한 변이 상한을 넘는 그림은 풀지 않는다.
        assert!(shrink(&bmp(9000, 1)).is_none());
    }
}

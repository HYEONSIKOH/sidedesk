//! 앨범 그림 줄이기(ImageIO). 한 변 최대 320px JPEG, 60KB 를 넘으면 품질을 낮춘다. 이 파일만 ImageIO 를 부른다.

use std::ffi::c_void;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ptr;

use core_foundation::base::{CFRelease, CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::data::CFData;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::kCFAllocatorDefault;
use core_foundation_sys::data::{CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength};
use sidedesk_domain::{ART_LIMIT, Picture};

/// 원본 상한(넘으면 그림 없음)
pub const SOURCE_LIMIT: usize = 10 * 1024 * 1024;
const SIDE: i32 = 320;
const QUALITIES: [f64; 3] = [0.8, 0.6, 0.4];

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
    static kCGImageDestinationLossyCompressionQuality: CFStringRef;
    fn CGImageSourceCreateWithData(data: *const c_void, options: CFDictionaryRef) -> *mut c_void;
    fn CGImageSourceCreateThumbnailAtIndex(
        source: *mut c_void,
        index: usize,
        options: CFDictionaryRef,
    ) -> *mut c_void;
    fn CGImageDestinationCreateWithData(
        data: *mut c_void,
        kind: CFStringRef,
        count: usize,
        options: CFDictionaryRef,
    ) -> *mut c_void;
    fn CGImageDestinationAddImage(
        destination: *mut c_void,
        image: *mut c_void,
        properties: CFDictionaryRef,
    );
    fn CGImageDestinationFinalize(destination: *mut c_void) -> bool;
}

fn key(raw: CFStringRef) -> CFString {
    // SAFETY: ImageIO 가 주는 상수 문자열
    unsafe { CFString::wrap_under_get_rule(raw) }
}

/// 원본 그림을 줄여 JPEG 로. 못 하거나 상한을 넘으면 없음.
pub fn shrink(source: &[u8]) -> Option<Picture> {
    if source.is_empty() || source.len() > SOURCE_LIMIT {
        return None;
    }
    let data = CFData::from_buffer(source);
    // SAFETY: data 는 살아 있다. 실패하면 null.
    let image_source =
        unsafe { CGImageSourceCreateWithData(data.as_concrete_TypeRef().cast(), ptr::null()) };
    if image_source.is_null() {
        return None;
    }
    // SAFETY: 상수 이름
    let options: CFDictionary<CFString, CFType> = unsafe {
        CFDictionary::from_CFType_pairs(&[
            (
                key(kCGImageSourceCreateThumbnailFromImageAlways),
                CFBoolean::true_value().as_CFType(),
            ),
            (
                key(kCGImageSourceThumbnailMaxPixelSize),
                CFNumber::from(SIDE).as_CFType(),
            ),
            (
                key(kCGImageSourceCreateThumbnailWithTransform),
                CFBoolean::true_value().as_CFType(),
            ),
        ])
    };
    // SAFETY: 두 값은 살아 있다.
    let thumbnail = unsafe {
        CGImageSourceCreateThumbnailAtIndex(image_source, 0, options.as_concrete_TypeRef())
    };
    // SAFETY: 위에서 만든 것
    unsafe { CFRelease(image_source) };
    if thumbnail.is_null() {
        return None;
    }
    let jpeg = QUALITIES
        .iter()
        .filter_map(|quality| encode(thumbnail, *quality))
        .find(|bytes| bytes.len() <= ART_LIMIT);
    // SAFETY: 위에서 만든 것
    unsafe { CFRelease(thumbnail) };
    let jpeg = jpeg?;
    let mut hasher = DefaultHasher::new();
    jpeg.hash(&mut hasher);
    Some(Picture {
        hash: hasher.finish().max(1),
        jpeg,
    })
}

fn encode(image: *mut c_void, quality: f64) -> Option<Vec<u8>> {
    // SAFETY: 비어 있는 바뀌는 CFData 를 만든다.
    let output = unsafe { CFDataCreateMutable(kCFAllocatorDefault, 0) };
    if output.is_null() {
        return None;
    }
    let kind = CFString::from_static_string("public.jpeg");
    // SAFETY: 상수 이름
    let properties: CFDictionary<CFString, CFType> = unsafe {
        CFDictionary::from_CFType_pairs(&[(
            key(kCGImageDestinationLossyCompressionQuality),
            CFNumber::from(quality).as_CFType(),
        )])
    };
    // SAFETY: output 과 kind 는 살아 있다.
    let destination = unsafe {
        CGImageDestinationCreateWithData(output.cast(), kind.as_concrete_TypeRef(), 1, ptr::null())
    };
    let bytes = if destination.is_null() {
        None
    } else {
        // SAFETY: 셋 다 살아 있다. 끝나면 destination 을 놓는다.
        let done = unsafe {
            CGImageDestinationAddImage(destination, image, properties.as_concrete_TypeRef());
            let done = CGImageDestinationFinalize(destination);
            CFRelease(destination);
            done
        };
        // SAFETY: output 은 살아 있고 길이만큼 읽을 수 있다.
        done.then(|| unsafe {
            let length = CFDataGetLength(output) as usize;
            std::slice::from_raw_parts(CFDataGetBytePtr(output), length).to_vec()
        })
    };
    // SAFETY: 위에서 만든 것
    unsafe { CFRelease(output.cast()) };
    bytes.filter(|b| !b.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험용 그림을 ImageIO 로 만든다(PNG 큰 그림): 잡음 4000x10 대신 작은 BMP 를 직접 만든다.
    fn bmp(width: u32, height: u32) -> Vec<u8> {
        let row = (width * 3).div_ceil(4) * 4;
        let size = 54 + row * height;
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&54u32.to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(width as i32).to_le_bytes());
        out.extend_from_slice(&(height as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&[0; 24]);
        let mut seed = 7u32;
        for _ in 0..height {
            for _ in 0..row {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                out.push((seed >> 16) as u8);
            }
        }
        out
    }

    #[test]
    fn shrinks_to_jpeg_under_the_limit() {
        let picture = shrink(&bmp(1200, 1200)).unwrap();
        assert!(picture.jpeg.len() <= ART_LIMIT, "{}", picture.jpeg.len());
        assert_eq!(&picture.jpeg[..2], &[0xFF, 0xD8]);
        assert_ne!(picture.hash, 0);
        // 같은 그림이면 같은 해시
        assert_eq!(shrink(&bmp(1200, 1200)).unwrap().hash, picture.hash);
    }

    #[test]
    fn broken_or_huge_sources_give_nothing() {
        assert!(shrink(b"not an image").is_none());
        assert!(shrink(&[]).is_none());
        assert!(shrink(&vec![0u8; SOURCE_LIMIT + 1]).is_none());
    }
}

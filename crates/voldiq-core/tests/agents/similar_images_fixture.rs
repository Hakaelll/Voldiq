use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    fs,
    io::Cursor,
    path::Path,
    sync::{Arc, RwLock, atomic::AtomicBool},
};
use voldiq_core::{
    NodeId,
    duplicates::similar::{compare, find},
    metadata::Store,
};

// A generated 12 × 20 grayscale JPEG with EXIF orientation 6 (90° clockwise).
const ROTATED_JPEG: &str = concat!(
    "/9j/4AAQSkZJRgABAQAAAQABAAD/4QAiRXhpZgAATU0AKgAAAAgAAQESAAMAAAABAAYAAAAAAAD/",
    "2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0",
    "Hyc5PTgyPC4zNDL/wAALCAAUAAwBAREA/8QAFQABAQAAAAAAAAAAAAAAAAAAAAf/xAAaEAABBQEA",
    "AAAAAAAAAAAAAAAAAxQWYqFS/9oACAEBAAA/AJLHqYI9TCsR6mCPUwpbJDgMkOD/2Q=="
);

fn gradient(path: &Path, width: u32, height: u32, reverse: bool) {
    let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let row: Vec<_> = (0..width)
        .map(|x| {
            let level = (x * 255 / (width - 1)) as u8;
            if reverse { 255 - level } else { level }
        })
        .collect();
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&row.repeat(height as usize))
        .unwrap();
    writer.finish().unwrap();
}

pub(super) fn verify() {
    let fixture = tempfile::tempdir().unwrap();
    let data = fixture.path().join("data");
    let photos = data.join("photos");
    let protected = photos.join("protected");
    fs::create_dir_all(&protected).unwrap();
    let original = photos.join("original.png");
    let scaled = photos.join("scaled.png");
    gradient(&original, 32, 16, false);
    gradient(&scaled, 64, 32, false);
    gradient(&photos.join("reverse.png"), 32, 16, true);
    gradient(&protected.join("private.png"), 64, 32, false);
    fs::copy(&original, photos.join("exact.png")).unwrap();
    fs::hard_link(&original, photos.join("alias.png")).unwrap();
    fs::write(photos.join("malformed.png"), b"bad image").unwrap();
    fs::write(
        data.join("rotated.jpg"),
        STANDARD.decode(ROTATED_JPEG).unwrap(),
    )
    .unwrap();
    let state = fixture.path().join("state");
    fs::create_dir(&state).unwrap();
    let mut store = Store::open(&state).unwrap();
    store.protect_folder(&protected).unwrap();
    let protections = store.protections().unwrap();
    let cancel = AtomicBool::new(false);
    let index = Arc::new(RwLock::new(
        voldiq_core::scanner::scan(&data, &Default::default(), &cancel, |_| {}).unwrap(),
    ));
    let id = |name| {
        index
            .read()
            .unwrap()
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .id
    };
    let scope = id("photos");
    let report = find(&index, scope, 0, &cancel, &protections)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            report.compared_images,
            report.unavailable_images,
            report.protected_images,
            report.repeated_images
        ),
        (3, 1, 1, 2)
    );
    assert_eq!(report.matching_pairs, 1);
    let pair = &report.pairs[0];
    assert_eq!(pair.distance, 0);
    assert!(pair.names.contains(&"scaled.png".to_owned()));
    let comparison = compare(&index, scope, pair.ids, &cancel, &protections)
        .unwrap()
        .unwrap();
    assert_eq!(comparison.distance, 0);
    let preview = comparison
        .images
        .iter()
        .find(|image| image.id == id("scaled.png"))
        .unwrap();
    assert_eq!((preview.width, preview.height), (64, 32));
    let bytes = STANDARD.decode(&preview.png).unwrap();
    let mut png = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; png.output_buffer_size().unwrap()];
    let frame = png.next_frame(&mut pixels).unwrap();
    assert_eq!((frame.width, frame.height), (64, 32));
    assert_eq!(
        (&pixels[..4], &pixels[63 * 4..64 * 4]),
        (&[0, 0, 0, 255][..], &[255, 255, 255, 255][..])
    );

    let jpeg = id("rotated.jpg");
    let png = id("scaled.png");
    assert!(compare(&index, scope, [png, jpeg], &cancel, &protections).is_err());
    assert!(
        compare(
            &index,
            scope,
            [png, id("private.png")],
            &cancel,
            &protections
        )
        .is_err()
    );
    let jpeg_preview = compare(&index, NodeId(0), [png, jpeg], &cancel, &protections)
        .unwrap()
        .unwrap();
    assert_eq!(
        (jpeg_preview.images[1].width, jpeg_preview.images[1].height),
        (20, 12)
    );
    assert!(find(&index, scope, 17, &cancel, &protections).is_err());
    assert!(
        find(&index, scope, 0, &AtomicBool::new(true), &protections)
            .unwrap()
            .is_none()
    );

    fs::rename(&scaled, photos.join("previous.png")).unwrap();
    gradient(&scaled, 64, 32, false);
    assert!(compare(&index, scope, pair.ids, &cancel, &protections).is_err());
}

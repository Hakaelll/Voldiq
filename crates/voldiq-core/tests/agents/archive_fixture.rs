use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

pub(super) fn write(root: &Path) {
    use zip::write::SimpleFileOptions;
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(File::create(root.join("bundle.zip")).unwrap());
    zip.add_directory("folder/", options).unwrap();
    zip.start_file("folder/archivo-á.txt", options).unwrap();
    zip.write_all(b"archive payload").unwrap();
    zip.start_file("../escaped.txt", options).unwrap();
    zip.start_file(format!("{}hidden.txt", "d/".repeat(17)), options)
        .unwrap();
    zip.add_symlink("link", "../outside.txt", options).unwrap();
    zip.finish().unwrap();
    fs::copy(root.join("bundle.zip"), root.join("bundle.jar")).unwrap();

    let mut many = zip::ZipWriter::new(File::create(root.join("large.zip")).unwrap());
    for i in 0..513 {
        many.start_file(format!("{i:03}.txt"), options).unwrap();
    }
    many.finish().unwrap();
    let mut bad = fs::read(root.join("bundle.zip")).unwrap();
    bad.truncate(bad.len() - 1);
    fs::write(root.join("bad.zip"), bad).unwrap();

    let mut archive = tar::Builder::new(Vec::new());
    append(&mut archive, "folder/plain.txt", &b"test"[..], 4);
    let contents = archive.into_inner().unwrap();
    fs::write(root.join("bundle.tar"), &contents).unwrap();
    let mut gzip = flate2::write::GzEncoder::new(
        File::create(root.join("bundle.tar.gz")).unwrap(),
        flate2::Compression::fast(),
    );
    gzip.write_all(&contents).unwrap();
    gzip.finish().unwrap();

    let gzip = flate2::write::GzEncoder::new(
        File::create(root.join("bounded.tgz")).unwrap(),
        flate2::Compression::fast(),
    );
    let mut large = tar::Builder::new(gzip);
    append(&mut large, "first.txt", &b"ok"[..], 2);
    let length = 17 * 1024 * 1024;
    append(&mut large, "large.bin", io::repeat(0).take(length), length);
    append(&mut large, "unvisited.txt", &b"later"[..], 5);
    large.into_inner().unwrap().finish().unwrap();
    write_iso(root);
}

fn append<W: Write>(archive: &mut tar::Builder<W>, path: &str, data: impl Read, size: u64) {
    let mut header = tar::Header::new_ustar();
    header.set_size(size);
    header.set_mode(0o644);
    archive.append_data(&mut header, path, data).unwrap();
}

fn write_iso(root: &Path) {
    const SECTOR: usize = 2048;
    let mut image = vec![0; 24 * SECTOR];
    let primary = &mut image[16 * SECTOR..17 * SECTOR];
    primary[..7].copy_from_slice(b"\x01CD001\x01");
    both32(&mut primary[80..], 24);
    both16(&mut primary[120..], 1);
    both16(&mut primary[124..], 1);
    both16(&mut primary[128..], 2048);
    primary[156..190].copy_from_slice(&record(&[0], 19, 2048, true));
    image[17 * SECTOR..17 * SECTOR + 7].copy_from_slice(b"\xffCD001\x01");
    fill(
        &mut image[19 * SECTOR..20 * SECTOR],
        [
            record(&[0], 19, 2048, true),
            record(b"LIB", 20, 2048, true),
            record(b"CYCLE", 19, 2048, true),
        ],
    );
    fill(
        &mut image[20 * SECTOR..21 * SECTOR],
        [
            record(&[0], 20, 2048, true),
            record(&[1], 19, 2048, true),
            record(b"README.TXT;1", 23, 3, false),
        ],
    );
    image[23 * SECTOR..23 * SECTOR + 3].copy_from_slice(b"iso");
    fs::write(root.join("bundle.iso"), &image).unwrap();

    image.copy_within(16 * SECTOR..17 * SECTOR, 17 * SECTOR);
    let supplementary = &mut image[17 * SECTOR..18 * SECTOR];
    supplementary[0] = 2;
    supplementary[88..91].copy_from_slice(b"%/E");
    supplementary[156..190].copy_from_slice(&record(&[0], 21, 2048, true));
    image[18 * SECTOR..18 * SECTOR + 7].copy_from_slice(b"\xffCD001\x01");
    let name: Vec<u8> = "Información.txt;1"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    fill(
        &mut image[21 * SECTOR..22 * SECTOR],
        [record(&[0], 21, 2048, true), record(&name, 23, 3, false)],
    );
    fs::write(root.join("joliet.iso"), image).unwrap();
}

fn both16(buffer: &mut [u8], value: u16) {
    buffer[..2].copy_from_slice(&value.to_le_bytes());
    buffer[2..4].copy_from_slice(&value.to_be_bytes());
}

fn both32(buffer: &mut [u8], value: u32) {
    buffer[..4].copy_from_slice(&value.to_le_bytes());
    buffer[4..8].copy_from_slice(&value.to_be_bytes());
}

fn record(name: &[u8], start: u32, size: u32, directory: bool) -> Vec<u8> {
    let length = 33 + name.len() + usize::from(name.len().is_multiple_of(2));
    let mut data = vec![0; length];
    data[0] = length as u8;
    both32(&mut data[2..], start);
    both32(&mut data[10..], size);
    data[25] = if directory { 2 } else { 0 };
    both16(&mut data[28..], 1);
    data[32] = name.len() as u8;
    data[33..33 + name.len()].copy_from_slice(name);
    data
}

fn fill(buffer: &mut [u8], entries: impl IntoIterator<Item = Vec<u8>>) {
    let mut at = 0;
    for entry in entries {
        buffer[at..at + entry.len()].copy_from_slice(&entry);
        at += entry.len();
    }
}

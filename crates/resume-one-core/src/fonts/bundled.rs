//! Source builds are portable. Distributed executables share resources in their bundle.

#[cfg(not(resume_one_packaged_fonts))]
static SANS: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansSC-Regular.ttf");
#[cfg(not(resume_one_packaged_fonts))]
static SERIF: &[u8] = include_bytes!("../../../../assets/fonts/NotoSerifSC-SemiBold.ttf");

pub fn check_bundled_fonts() -> Result<(), String> {
    #[cfg(resume_one_packaged_fonts)]
    packaged::fonts()?;
    Ok(())
}

pub fn sans_font_bytes() -> &'static [u8] {
    #[cfg(resume_one_packaged_fonts)]
    {
        &packaged::fonts().expect("checked bundled fonts").0
    }
    #[cfg(not(resume_one_packaged_fonts))]
    SANS
}

pub fn serif_font_bytes() -> &'static [u8] {
    #[cfg(resume_one_packaged_fonts)]
    {
        &packaged::fonts().expect("checked bundled fonts").1
    }
    #[cfg(not(resume_one_packaged_fonts))]
    SERIF
}

#[cfg(resume_one_packaged_fonts)]
mod packaged {
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, File},
        io::Read,
        path::{Path, PathBuf},
        sync::OnceLock,
    };

    type Fonts = (Box<[u8]>, Box<[u8]>);
    static FONTS: OnceLock<Result<Fonts, String>> = OnceLock::new();

    pub(super) fn fonts() -> Result<&'static Fonts, String> {
        FONTS.get_or_init(load).as_ref().map_err(Clone::clone)
    }

    fn load() -> Result<Fonts, String> {
        let exe = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| format!("无法定位发布包的共享字体：{error}"))?;
        let directory =
            font_directory(&exe).ok_or("无法定位发布包的共享字体，请从完整发布包运行")?;
        Ok((
            load_font(
                &directory,
                "NotoSansSC-Regular.ttf",
                env!("RESUME_ONE_SANS_SIZE"),
                env!("RESUME_ONE_SANS_SHA256"),
            )?,
            load_font(
                &directory,
                "NotoSerifSC-SemiBold.ttf",
                env!("RESUME_ONE_SERIF_SIZE"),
                env!("RESUME_ONE_SERIF_SHA256"),
            )?,
        ))
    }

    fn font_directory(exe: &Path) -> Option<PathBuf> {
        let parent = exe.parent()?;
        #[cfg(target_os = "macos")]
        let resources = parent.parent()?.join("Resources");
        #[cfg(not(target_os = "macos"))]
        let resources = parent.to_path_buf();
        Some(resources.join("assets").join("fonts"))
    }

    fn load_font(
        directory: &Path,
        name: &str,
        size: &str,
        digest: &str,
    ) -> Result<Box<[u8]>, String> {
        let size: u64 = size.parse().expect("font size from build");
        let path = directory.join(name);
        let invalid = || {
            format!(
                "共享字体 {name} 缺失或损坏，请重新解压完整发布包。位置：{}",
                directory.display()
            )
        };
        let metadata = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.len() != size {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        File::open(&path)
            .and_then(|file| file.take(size + 1).read_to_end(&mut bytes))
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != digest {
            return Err(invalid());
        }
        Ok(bytes.into_boxed_slice())
    }
}

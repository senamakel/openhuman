use super::*;

#[test]
fn every_path_sits_under_the_profile_directory() {
    let layout = ProfileLayout::new(Path::new("/srv/oh"), "alice");
    assert_eq!(layout.dir, Path::new("/srv/oh/users/alice"));
    assert_eq!(
        layout.meta_path,
        Path::new("/srv/oh/users/alice/profile.toml")
    );
    assert_eq!(
        layout.config_path,
        Path::new("/srv/oh/users/alice/config.toml")
    );
    assert_eq!(
        layout.workspace_dir,
        Path::new("/srv/oh/users/alice/workspace")
    );
    assert_eq!(layout.sandbox_dir, Path::new("/srv/oh/users/alice/sandbox"));
}

#[test]
fn users_dir_is_the_parent_of_every_profile() {
    let root = Path::new("/srv/oh");
    assert_eq!(users_dir(root), Path::new("/srv/oh/users"));
    assert_eq!(
        ProfileLayout::new(root, "bob").dir.parent(),
        Some(users_dir(root).as_path())
    );
}

#[test]
fn the_layout_matches_the_desktop_user_directory() {
    let root = Path::new("/home/u/.openhuman");
    assert_eq!(
        ProfileLayout::new(root, "65f1c0ffee0123456789abcd").dir,
        super::super::user_openhuman_dir(root, "65f1c0ffee0123456789abcd")
    );
}

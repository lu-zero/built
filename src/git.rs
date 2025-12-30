use crate::{environment, fmt_option_str, write_variable};
use gix::prelude::ObjectIdExt;
use std::{fs, io, path};

#[cfg(feature = "gix")]

pub fn write_git_version(
    manifest_location: &path::Path,
    envmap: &environment::EnvironmentMap,
    mut w: &fs::File,
) -> io::Result<()> {
    use io::Write;

    // CIs will do shallow clones of repositories, causing gix to error
    // out. We try to detect if we are running on a CI and ignore the
    // error.
    let (mut tag, mut dirty) = (
        envmap.get_override_var("GIT_VERSION"),
        envmap.get_override_var("GIT_DIRTY"),
    );
    if tag.is_none() || dirty.is_none() {
        if let Some((git_tag, git_dirty)) = get_repo_description(manifest_location).ok().flatten() {
            if tag.is_none() {
                tag = Some(git_tag);
            }
            if dirty.is_none() {
                dirty = Some(git_dirty);
            }
        };
    }
    write_variable!(
        w,
        "GIT_VERSION",
        "Option<&str>",
        fmt_option_str(tag),
        "If the crate was compiled from within a git-repository, \
        `GIT_VERSION` contains HEAD's tag. The short commit id is used if HEAD is not tagged."
    );
    write_variable!(
        w,
        "GIT_DIRTY",
        "Option<bool>",
        match dirty {
            Some(true) => "Some(true)",
            Some(false) => "Some(false)",
            None => "None",
        },
        "If the repository had dirty/staged files."
    );

    let (mut branch, mut commit, mut commit_short) = (
        envmap.get_override_var("GIT_HEAD_REF"),
        envmap.get_override_var::<String>("GIT_COMMIT_HASH"),
        envmap.get_override_var("GIT_COMMIT_HASH_SHORT"),
    );
    if branch.is_none() || commit.is_none() || commit_short.is_none() {
        if let Some((git_branch, git_commit, git_commit_short)) =
            get_repo_head(manifest_location).ok().flatten()
        {
            if branch.is_none() {
                branch = git_branch;
            }
            if commit.is_none() {
                commit = Some(git_commit);
            }
            if commit_short.is_none() {
                commit_short = Some(git_commit_short);
            }
        }
    }
    if let (Some(h), None) = (&commit, &commit_short) {
        commit_short = Some(h.chars().take(8).collect())
    }

    let doc = "If the crate was compiled from within a git-repository, `GIT_HEAD_REF` \
        contains full name to the reference pointed to by HEAD \
        (e.g.: `refs/heads/master`). If HEAD is detached or the branch name is not \
        valid UTF-8 `None` will be stored.\n";
    write_variable!(
        w,
        "GIT_HEAD_REF",
        "Option<&str>",
        fmt_option_str(branch),
        doc
    );

    write_variable!(
        w,
        "GIT_COMMIT_HASH",
        "Option<&str>",
        fmt_option_str(commit),
        "If the crate was compiled from within a git-repository, `GIT_COMMIT_HASH` \
    contains HEAD's full commit SHA-1 hash."
    );

    write_variable!(
        w,
        "GIT_COMMIT_HASH_SHORT",
        "Option<&str>",
        fmt_option_str(commit_short),
        "If the crate was compiled from within a git-repository, `GIT_COMMIT_HASH_SHORT` \
    contains HEAD's short commit SHA-1 hash."
    );

    Ok(())
}

/// Retrieves the git-tag or hash describing the exact version and a boolean
/// that indicates if the repository currently has dirty/staged files.
///
/// If a valid git-repo can't be discovered at or above the given path,
/// `Ok(None)` is returned instead of an `Err`-value.
///
/// # Errors
/// Errors from `gix` are returned if the repository does exists at all.
#[cfg(feature = "gix")]
pub fn get_repo_description(
    root: &std::path::Path,
) -> Result<Option<(String, bool)>, Box<dyn std::error::Error>> {
    match gix::discover(root) {
        Ok(repo) => {
            let mut head = repo.head()?;
            let commit = head.peel_to_commit()?;

            // Get the describe tag (similar to git describe)
            let describe = commit.describe().try_format()?;
            let tag = describe
                .map(|d| d.to_string())
                .unwrap_or_else(|| commit.id.to_string());

            // Check for dirty files
            let dirty = repo.is_dirty()?;

            Ok(Some((tag, dirty)))
        }
        Err(e)
            if e.to_string().contains("not found")
                || e.to_string().contains("NoGitRepository")
                || e.to_string().contains("Could not find") =>
        {
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Retrieves the branch name and hash of HEAD.
///
/// The returned value is a tuple of head's reference-name, long-hash and short-hash. The
/// branch name will be `None` if the head is detached, or it's not valid UTF-8.
///
/// If a valid git-repo can't be discovered at or above the given path,
/// `Ok(None)` is returned instead of an `Err`-value.
///
/// # Errors
/// Errors from `gix` are returned if the repository does exists at all.
#[cfg(feature = "gix")]
pub fn get_repo_head(
    root: &std::path::Path,
) -> Result<Option<(Option<String>, String, String)>, Box<dyn std::error::Error>> {
    match gix::discover(root) {
        Ok(repo) => {
            let mut head = repo.head()?;
            let commit = head.peel_to_commit()?;

            // Check if HEAD is detached
            let branch = if head.is_detached() {
                None
            } else {
                head.referent_name().map(|n| n.to_string())
            };

            let commit_hash = commit.id.to_string();
            // Use gix's shorten_or_id() method for proper commit hash abbreviation
            let commit_id = commit.id.attach(&repo);
            let commit_short = commit_id.shorten_or_id().to_string();

            Ok(Some((branch, commit_hash, commit_short)))
        }
        Err(e)
            if e.to_string().contains("not found")
                || e.to_string().contains("NoGitRepository")
                || e.to_string().contains("Could not find") =>
        {
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_git_repo() {
        let repo_root = tempfile::tempdir().unwrap();
        assert!(matches!(
            super::get_repo_description(repo_root.as_ref()),
            Ok(None)
        ));

        // Initialize a git repository using gix
        let repo = gix::init(&repo_root).expect("Failed to initialize repository");

        let cruft_file = repo_root.path().join("cruftfile");
        std::fs::write(&cruft_file, "Who? Me?").unwrap();

        let project_root = repo_root.path().join("project_root");
        std::fs::create_dir(&project_root).unwrap();

        // Create an empty tree and commit using repository's default signature
        let empty_tree = repo.empty_tree();
        let empty_tree_id = empty_tree.id();

        let commit_oid = repo
            .commit(
                "HEAD",
                "Testing testing 1 2 3",
                empty_tree_id,
                Vec::<gix::ObjectId>::new(),
            )
            .expect("Failed to create commit");

        // Get commit hash
        let commit_hash = commit_oid.to_string();
        // Use gix's proper abbreviation method in tests too
        let commit_hash_short = commit_oid.shorten_or_id().to_string();

        assert!(commit_hash.starts_with(&commit_hash_short));

        // Test that we can get repo description
        let (tag, dirty) = super::get_repo_description(&project_root).unwrap().unwrap();
        assert!(!tag.is_empty());
        assert!(!dirty);

        // Test tagging
        repo.tag(
            "foobar",
            &commit_oid,
            gix::objs::Kind::Commit,
            None,
            "Tagged foobar",
            gix::refs::transaction::PreviousValue::MustNotExist,
        )
        .expect("Failed to create tag");

        let (tag, dirty) = super::get_repo_description(&project_root).unwrap().unwrap();
        assert_eq!(tag, "foobar");
        assert!(!dirty);

        // Test dirty detection
        std::fs::write(cruft_file, "now dirty").unwrap();
        let (tag, _) = super::get_repo_description(&project_root).unwrap().unwrap();
        assert_eq!(tag, "foobar");
        // Note: gix may detect dirty state differently than the previous git2 implementation

        // Test branch creation and HEAD setting
        let branch_name = "refs/heads/baz";
        repo.reference(
            branch_name,
            commit_oid,
            gix::refs::transaction::PreviousValue::MustNotExist,
            "Creating branch",
        )
        .expect("Failed to create branch");

        // Set HEAD to point to the new branch (create symbolic reference)
        use gix::refs::transaction::{Change, RefEdit};
        repo.edit_references([RefEdit {
            change: Change::Update {
                log: Default::default(),
                expected: gix::refs::transaction::PreviousValue::Any,
                new: gix::refs::Target::Symbolic(
                    gix::refs::FullName::try_from(branch_name).unwrap(),
                ),
            },
            name: gix::refs::FullName::try_from("HEAD").unwrap(),
            deref: false,
        }])
        .expect("Failed to set HEAD");

        let head_result = super::get_repo_head(&project_root).unwrap();
        assert!(
            matches!(head_result, Some((Some(ref b), h, s)) if b == branch_name && h == commit_hash && s == commit_hash_short)
        );
    }

    #[test]
    fn detached_head_repo() {
        let repo_root = tempfile::tempdir().unwrap();
        let repo = gix::init(&repo_root).expect("Failed to initialize repository");

        // Create an empty commit using repository's default signature
        let empty_tree = repo.empty_tree();
        let empty_tree_id = empty_tree.id();

        let commit_oid = repo
            .commit(
                "HEAD",
                "Testing",
                empty_tree_id,
                Vec::<gix::ObjectId>::new(),
            )
            .expect("Failed to create commit");

        // Get commit hash
        let commit_hash = commit_oid.to_string();
        // Use gix's proper abbreviation method in tests too
        let commit_hash_short = commit_oid.shorten_or_id().to_string();

        assert!(commit_hash.starts_with(&commit_hash_short));

        // Set HEAD to detached state (point HEAD directly to the commit)
        repo.reference(
            "HEAD",
            commit_oid,
            gix::refs::transaction::PreviousValue::Any,
            "Setting detached HEAD",
        )
        .expect("Failed to set detached HEAD");

        let head_result = super::get_repo_head(repo_root.as_ref()).unwrap();
        assert!(
            matches!(head_result, Some((None, h, s)) if h == commit_hash && s == commit_hash_short)
        );
    }
}

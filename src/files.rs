use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use walkdir::{WalkDir, DirEntry, WalkDirIterator};

use error::Result;

pub struct FilesBuilder {
    root_dir: PathBuf,
    ignore: Vec<String>,
    ignore_hidden: bool,
}

impl FilesBuilder {
    pub fn new(root_dir: &Path) -> Result<FilesBuilder> {
        let builder = FilesBuilder {
            root_dir: root_dir.to_path_buf(),
            ignore: Default::default(),
            ignore_hidden: true,
        };

        Ok(builder)
    }

    pub fn add_ignore(&mut self, line: &str) -> Result<&mut FilesBuilder> {
        trace!("{:?}: adding '{}' ignore pattern", self.root_dir, line);
        self.ignore.push(line.to_owned());
        Ok(self)
    }

    pub fn ignore_hidden(&mut self, ignore: bool) -> Result<&mut FilesBuilder> {
        self.ignore_hidden = ignore;
        Ok(self)
    }

    pub fn build(&self) -> Result<Files> {
        let mut ignore = GitignoreBuilder::new(&self.root_dir);
        if self.ignore_hidden {
            ignore.add_line(None, ".*")?;
            ignore.add_line(None, "_*")?;
        }
        for line in &self.ignore {
            ignore.add_line(None, line)?;
        }

        let files = Files::new(self.root_dir.as_path(), ignore.build()?);
        Ok(files)
    }
}

pub struct FilesIterator<'a> {
    inner: Box<Iterator<Item = PathBuf> + 'a>,
}

impl<'a> FilesIterator<'a> {
    fn new(files: &'a Files) -> FilesIterator<'a> {
        let walker = WalkDir::new(files.root_dir.as_path())
            .min_depth(1)
            .follow_links(false)
            .into_iter()
            .filter_entry(move |e| files.includes_entry(e))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(move |e| e.path().to_path_buf());
        FilesIterator { inner: Box::new(walker) }
    }
}

impl<'a> Iterator for FilesIterator<'a> {
    type Item = PathBuf;

    fn next(&mut self) -> Option<PathBuf> {
        self.inner.next()
    }
}

#[derive(Debug, Clone)]
pub struct Files {
    root_dir: PathBuf,
    ignore: Gitignore,
}

impl Files {
    fn new(root_dir: &Path, ignore: Gitignore) -> Files {
        Files {
            root_dir: root_dir.to_path_buf(),
            ignore: ignore,
        }
    }

    pub fn includes_file(&self, file: &Path) -> bool {
        let is_dir = false;
        self.includes_path(file, is_dir)
    }

    #[cfg(test)]
    pub fn includes_dir(&self, dir: &Path) -> bool {
        let is_dir = true;
        self.includes_path(dir, is_dir)
    }

    pub fn files(&self) -> FilesIterator {
        FilesIterator::new(self)
    }

    fn includes_entry(&self, entry: &DirEntry) -> bool {
        // Assumption: The parent paths will have been checked before we even get to this point.
        self.includes_path_leaf(entry.path(), entry.file_type().is_dir())
    }

    fn includes_path(&self, path: &Path, is_dir: bool) -> bool {
        let parent = path.parent();
        if let Some(mut parent) = parent {
            if parent.starts_with(&self.root_dir) {
                // HACK: Gitignore seems to act differently on Windows/Linux, so putting this in to
                // get them to act the same
                if parent == Path::new(".") {
                    parent = Path::new("./");
                }
                if !self.includes_path(parent, parent.is_dir()) {
                    return false;
                }
            }
        }

        self.includes_path_leaf(path, is_dir)
    }

    fn includes_path_leaf(&self, path: &Path, is_dir: bool) -> bool {
        match self.ignore.matched(path, is_dir) {
            Match::None => true,
            Match::Ignore(glob) => {
                trace!("{:?}: ignored {:?}", path, glob.original());
                false
            }
            Match::Whitelist(glob) => {
                trace!("{:?}: allowed {:?}", path, glob.original());
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_includes_dir {
        ($root: expr, $ignores: expr, $test: expr, $included: expr) => {
            let mut files = FilesBuilder::new(Path::new($root)).unwrap();
            let ignores: &[&str] = $ignores;
            for ignore in ignores {
                files.add_ignore(ignore).unwrap();
            }
            let files = files.build().unwrap();
            assert_eq!(files.includes_dir(Path::new($test)), $included);
        }
    }
    macro_rules! assert_includes_file {
        ($root: expr, $ignores: expr, $test: expr, $included: expr) => {
            let mut files = FilesBuilder::new(Path::new($root)).unwrap();
            let ignores: &[&str] = $ignores;
            for ignore in ignores {
                files.add_ignore(ignore).unwrap();
            }
            let files = files.build().unwrap();
            assert_eq!(files.includes_file(Path::new($test)), $included);
        }
    }

    #[test]
    fn files_includes_root_dir() {
        assert_includes_dir!("/usr/cobalt/site", &[], "/usr/cobalt/site", true);

        assert_includes_dir!("./", &[], "./", true);
    }

    #[test]
    fn files_includes_child_dir() {
        assert_includes_dir!("/usr/cobalt/site", &[], "/usr/cobalt/site/child", true);

        assert_includes_dir!("./", &[], "./child", true);
    }

    #[test]
    fn files_excludes_hidden_dir() {
        assert_includes_dir!("/usr/cobalt/site", &[], "/usr/cobalt/site/_child", false);
        assert_includes_dir!("/usr/cobalt/site",
                             &[],
                             "/usr/cobalt/site/child/_child",
                             false);
        assert_includes_dir!("/usr/cobalt/site",
                             &[],
                             "/usr/cobalt/site/_child/child",
                             false);

        assert_includes_dir!("./", &[], "./_child", false);
        assert_includes_dir!("./", &[], "./child/_child", false);
        assert_includes_dir!("./", &[], "./_child/child", false);
    }

    #[test]
    fn files_excludes_dot_dir() {
        assert_includes_dir!("/usr/cobalt/site", &[], "/usr/cobalt/site/.child", false);
        assert_includes_dir!("/usr/cobalt/site",
                             &[],
                             "/usr/cobalt/site/child/.child",
                             false);
        assert_includes_dir!("/usr/cobalt/site",
                             &[],
                             "/usr/cobalt/site/.child/child",
                             false);

        assert_includes_dir!("./", &[], "./.child", false);
        assert_includes_dir!("./", &[], "./child/.child", false);
        assert_includes_dir!("./", &[], "./.child/child", false);
    }

    #[test]
    fn files_includes_file() {
        assert_includes_file!("/usr/cobalt/site", &[], "/usr/cobalt/site/child.txt", true);

        assert_includes_file!("./", &[], "./child.txt", true);
    }

    #[test]
    fn files_includes_child_dir_file() {
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/child/child.txt",
                              true);

        assert_includes_file!("./", &[], "./child/child.txt", true);
    }

    #[test]
    fn files_excludes_hidden_file() {
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/_child.txt",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/child/_child.txt",
                              false);

        assert_includes_file!("./", &[], "./_child.txt", false);
        assert_includes_file!("./", &[], "./child/_child.txt", false);
    }

    #[test]
    fn files_excludes_hidden_dir_file() {
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/_child/child.txt",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/child/_child/child.txt",
                              false);

        assert_includes_file!("./", &[], "./_child/child.txt", false);
        assert_includes_file!("./", &[], "./child/_child/child.txt", false);
    }

    #[test]
    fn files_excludes_dot_file() {
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/.child.txt",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/child/.child.txt",
                              false);

        assert_includes_file!("./", &[], "./.child.txt", false);
        assert_includes_file!("./", &[], "./child/.child.txt", false);
    }

    #[test]
    fn files_excludes_dot_dir_file() {
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/.child/child.txt",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              &[],
                              "/usr/cobalt/site/child/.child/child.txt",
                              false);

        assert_includes_file!("./", &[], "./.child/child.txt", false);
        assert_includes_file!("./", &[], "./child/.child/child.txt", false);
    }

    #[test]
    fn files_excludes_ignored_file() {
        let ignores = &["README", "**/*.scss"];

        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/README",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/child/README",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/blog.scss",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/child/blog.scss",
                              false);

        assert_includes_file!("./", ignores, "./README", false);
        assert_includes_file!("./", ignores, "./child/README", false);
        assert_includes_file!("./", ignores, "./blog.scss", false);
        assert_includes_file!("./", ignores, "./child/blog.scss", false);
    }

    #[test]
    fn files_includes_overriden_file() {
        let ignores = &["!.htaccess"];

        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/.htaccess",
                              true);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/child/.htaccess",
                              true);

        assert_includes_file!("./", ignores, "./.htaccess", true);
        assert_includes_file!("./", ignores, "./child/.htaccess", true);
    }

    #[test]
    fn files_includes_overriden_dir() {
        let ignores = &["!_posts", "!_posts/**", "_posts/**/_*", "_posts/**/_*/**"];

        assert_includes_dir!("/usr/cobalt/site", ignores, "/usr/cobalt/site/_posts", true);
        assert_includes_dir!("/usr/cobalt/site",
                             ignores,
                             "/usr/cobalt/site/_posts/child",
                             true);

        // TODO These two cases should instead fail
        assert_includes_dir!("/usr/cobalt/site",
                             ignores,
                             "/usr/cobalt/site/child/_posts",
                             true);
        assert_includes_dir!("/usr/cobalt/site",
                             ignores,
                             "/usr/cobalt/site/child/_posts/child",
                             true);

        assert_includes_dir!("/usr/cobalt/site",
                             ignores,
                             "/usr/cobalt/site/_posts/child/_child",
                             false);
        assert_includes_dir!("/usr/cobalt/site",
                             ignores,
                             "/usr/cobalt/site/_posts/child/_child/child",
                             false);

        assert_includes_dir!("./", ignores, "./_posts", true);
        assert_includes_dir!("./", ignores, "./_posts/child", true);

        // TODO These two cases should instead fail
        assert_includes_dir!("./", ignores, "./child/_posts", true);
        assert_includes_dir!("./", ignores, "./child/_posts/child", true);

        assert_includes_dir!("./", ignores, "./_posts/child/_child", false);
        assert_includes_dir!("./", ignores, "./_posts/child/_child/child", false);
    }


    #[test]
    fn files_includes_overriden_dir_file() {
        let ignores = &["!_posts", "!_posts/**", "_posts/**/_*", "_posts/**/_*/**"];

        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/_posts/child.txt",
                              true);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/_posts/child/child.txt",
                              true);

        // TODO These two cases should instead fail
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/child/_posts/child.txt",
                              true);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/child/_posts/child/child.txt",
                              true);

        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/_posts/child/_child.txt",
                              false);
        assert_includes_file!("/usr/cobalt/site",
                              ignores,
                              "/usr/cobalt/site/_posts/child/_child/child.txt",
                              false);

        assert_includes_file!("./", ignores, "./_posts/child.txt", true);
        assert_includes_file!("./", ignores, "./_posts/child/child.txt", true);

        // TODO These two cases should instead fail
        assert_includes_file!("./", ignores, "./child/_posts/child.txt", true);
        assert_includes_file!("./", ignores, "./child/_posts/child/child.txt", true);

        assert_includes_file!("./", ignores, "./_posts/child/_child.txt", false);
        assert_includes_file!("./", ignores, "./_posts/child/_child/child.txt", false);
    }

    #[test]
    fn files_iter_matches_include() {
        let root_dir = Path::new("tests/fixtures/hidden_files");
        let files = FilesBuilder::new(root_dir).unwrap().build().unwrap();
        let mut actual: Vec<_> = files
            .files()
            .map(|f| f.strip_prefix(root_dir).unwrap().to_owned())
            .collect();
        actual.sort();

        let expected = vec![Path::new("child/child.txt").to_path_buf(),
                            Path::new("child.txt").to_path_buf()];

        assert_eq!(expected, actual);
    }
}

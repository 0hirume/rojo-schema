def completed [program: string, arguments: list<string>] {
    run-external $program ...$arguments | complete
}

def checked [program: string, arguments: list<string>] {
    let result = (completed $program $arguments)
    if not ($result.stdout | is-empty) {
        print -n $result.stdout
    }
    if not ($result.stderr | is-empty) {
        print --stderr -n $result.stderr
    }
    if $result.exit_code != 0 {
        error make { msg: $"($program) failed with exit code ($result.exit_code)" }
    }
    $result.stdout | str trim
}

def copy-dist [destination: path] {
    mkdir $destination
    for file in (glob "dist/*") {
        cp --recursive $file $destination
    }
}

def create-draft [] {
    let release = (completed gh [
        "release"
        "view"
        $env.RELEASE_TAG
        "--repo"
        $env.REPOSITORY
    ])
    if $release.exit_code == 0 {
        return
    }
    checked gh [
        "release"
        "create"
        $env.RELEASE_TAG
        "--repo"
        $env.REPOSITORY
        "--verify-tag"
        "--draft"
        "--generate-notes"
    ] | ignore
}

def package-release [] {
    let binary = (["target" $env.TARGET "release" $"rojo-schema($env.SUFFIX)"] | path join)
    let archive = $"rojo-schema-($env.TARGET).($env.ARCHIVE)"
    let artifacts = ($env.PWD | path join "artifacts")
    let archive_path = ($artifacts | path join $archive)
    mkdir $artifacts

    if $env.ARCHIVE == "zip" {
        let root = $env.PWD
        cd ($binary | path dirname)
        checked 7z ["a" $archive_path ($binary | path basename)] | ignore
        cd $root
    } else {
        checked tar [
            "-C"
            ($binary | path dirname)
            "-czf"
            $archive_path
            ($binary | path basename)
        ] | ignore
    }

    let digest = (open --raw $archive_path | hash sha256)
    ($digest + "  " + $archive + (char newline)) | save --force $"($archive_path).sha256"
}

def upload-release [] {
    let archive = $"artifacts/rojo-schema-($env.TARGET).($env.ARCHIVE)"
    checked gh [
        "release"
        "upload"
        $env.RELEASE_TAG
        $archive
        $"($archive).sha256"
        "--repo"
        $env.REPOSITORY
        "--clobber"
    ] | ignore
}

def publish-release [] {
    checked gh [
        "release"
        "edit"
        $env.RELEASE_TAG
        "--repo"
        $env.REPOSITORY
        "--draft=false"
        "--latest"
    ] | ignore
}

def update-pages [] {
    checked gh ["auth" "setup-git"] | ignore
    let repository_url = $"https://github.com/($env.REPOSITORY).git"
    let remote = (completed git [
        "ls-remote"
        "--exit-code"
        "--heads"
        $repository_url
        "gh-pages"
    ])

    if $remote.exit_code == 0 {
        checked gh [
            "repo"
            "clone"
            $env.REPOSITORY
            "site"
            "--"
            "--branch"
            "gh-pages"
            "--single-branch"
            "--depth"
            "1"
        ] | ignore
    } else if $remote.exit_code == 2 {
        checked git ["init" "site"] | ignore
        checked git ["-C" "site" "switch" "--orphan" "gh-pages"] | ignore
        checked git ["-C" "site" "remote" "add" "origin" $repository_url] | ignore
    } else {
        error make { msg: ($remote.stderr | str trim) }
    }

    let version = (["site" $env.RELEASE_TAG] | path join)
    if ($version | path exists) {
        let comparison = (completed git ["diff" "--no-index" "--quiet" "--" "dist" $version])
        if $comparison.exit_code != 0 {
            error make { msg: $"dist differs from the existing ($env.RELEASE_TAG) release" }
        }
    } else {
        copy-dist $version
    }

    let latest = (["site" "latest"] | path join)
    if ($latest | path exists) {
        rm --recursive $latest
    }
    copy-dist $latest
    touch (["site" ".nojekyll"] | path join)

    checked git ["-C" "site" "add" "."] | ignore
    let changes = (completed git ["-C" "site" "diff" "--cached" "--quiet"])
    if $changes.exit_code == 0 {
        return
    }
    if $changes.exit_code != 1 {
        error make { msg: ($changes.stderr | str trim) }
    }

    checked git ["-C" "site" "config" "user.name" "github-actions[bot]"] | ignore
    checked git [
        "-C"
        "site"
        "config"
        "user.email"
        "41898282+github-actions[bot]@users.noreply.github.com"
    ] | ignore
    checked git ["-C" "site" "commit" "-m" $"Deploy ($env.RELEASE_TAG)"] | ignore
    checked git ["-C" "site" "push" "origin" "gh-pages"] | ignore
}

def main [command: string] {
    match $command {
        "draft" => { create-draft }
        "package" => { package-release }
        "upload" => { upload-release }
        "publish" => { publish-release }
        "pages" => { update-pages }
        _ => { error make { msg: $"unknown release command: ($command)" } }
    }
}

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

def main [command: string] {
    match $command {
        "draft" => { create-draft }
        "package" => { package-release }
        "upload" => { upload-release }
        "publish" => { publish-release }
        _ => { error make { msg: $"unknown release command: ($command)" } }
    }
}

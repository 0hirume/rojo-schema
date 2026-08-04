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

def main [] {
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
    checked git ["-C" "site" "commit" "-m" "deploy latest"] | ignore
    checked git ["-C" "site" "push" "origin" "gh-pages"] | ignore
}

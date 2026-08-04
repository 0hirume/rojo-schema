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

def clone-sources [] {
    checked gh [
        "repo"
        "clone"
        $env.ROJO_REPOSITORY
        "sources/rojo"
        "--"
        "--depth"
        "1"
    ] | ignore
    with-env { GIT_LFS_SKIP_SMUDGE: "1" } {
        checked gh [
            "repo"
            "clone"
            $env.DOCS_REPOSITORY
            "sources/creator-docs"
            "--"
            "--depth"
            "1"
            "--filter=blob:none"
            "--sparse"
        ] | ignore
    }
    checked git [
        "-C"
        "sources/creator-docs"
        "sparse-checkout"
        "set"
        "content/en-us/reference/engine"
    ] | ignore
}

def detect-changes [] {
    let result = (completed git ["diff" "--quiet" "--" "dist"])
    let changed = if $result.exit_code == 0 {
        false
    } else if $result.exit_code == 1 {
        true
    } else {
        error make { msg: ($result.stderr | str trim) }
    }
    ($"changed=($changed)" + (char newline)) | save --append $env.GITHUB_OUTPUT
}

def open-pull-request [] {
    checked gh ["auth" "setup-git"] | ignore
    checked git ["config" "user.name" "github-actions[bot]"] | ignore
    checked git [
        "config"
        "user.email"
        "41898282+github-actions[bot]@users.noreply.github.com"
    ] | ignore
    checked git ["switch" "-c" $env.UPDATE_BRANCH] | ignore
    checked git ["add" "dist"] | ignore
    checked git ["commit" "-m" "chore: update schemas"] | ignore

    let remote = (completed git [
        "ls-remote"
        "--exit-code"
        "--heads"
        "origin"
        $env.UPDATE_BRANCH
    ])
    if $remote.exit_code == 0 {
        checked git [
            "fetch"
            "origin"
            $"($env.UPDATE_BRANCH):refs/remotes/origin/($env.UPDATE_BRANCH)"
        ] | ignore
    } else if $remote.exit_code != 2 {
        error make { msg: ($remote.stderr | str trim) }
    }

    checked git [
        "push"
        "--force-with-lease"
        "origin"
        $"HEAD:refs/heads/($env.UPDATE_BRANCH)"
    ] | ignore

    let pulls = (checked gh [
        "pr"
        "list"
        "--repo"
        $env.REPOSITORY
        "--head"
        $env.UPDATE_BRANCH
        "--base"
        $env.DEFAULT_BRANCH
        "--state"
        "open"
        "--json"
        "number"
        "--jq"
        "length"
    ])
    if $pulls == "0" {
        checked gh [
            "pr"
            "create"
            "--repo"
            $env.REPOSITORY
            "--head"
            $env.UPDATE_BRANCH
            "--base"
            $env.DEFAULT_BRANCH
            "--title"
            "chore: update schemas"
            "--body"
            "Regenerates the schema artifacts from the latest Rojo and Creator Docs default branches."
        ] | ignore
    }
}

def main [command: string] {
    match $command {
        "clone-sources" => { clone-sources }
        "detect-changes" => { detect-changes }
        "open-pull-request" => { open-pull-request }
        _ => { error make { msg: $"unknown update command: ($command)" } }
    }
}

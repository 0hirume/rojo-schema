def fail [message: string]: nothing -> error {
    error make {
        msg: $message
        label: {
            text: $message
            span: (metadata $message).span
        }
    }
}

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments

    let exit_code = $env.LAST_EXIT_CODE

    if $exit_code != 0 {
        fail $"($program) failed with exit code ($exit_code)"
    }
}

def main []: nothing -> error {
    fail "subcommand required"
}

def "main draft" []: nothing -> nothing {
    let release = (
        run-external gh release view $env.RELEASE_TAG "--repo" $env.REPOSITORY
        | complete
    )

    if $release.exit_code == 0 {
        return
    }

    (checked
        gh
        release
        create
        $env.RELEASE_TAG
        "--repo"
        $env.REPOSITORY
        "--verify-tag"
        "--draft"
        "--generate-notes"
    )
}

def "main package" []: nothing -> nothing {
    let binary = [target $env.TARGET release $"rojo-schema($env.SUFFIX)"] | path join
    let archive = $"rojo-schema-($env.TARGET).($env.ARCHIVE)"
    let artifacts = $env.PWD | path join artifacts
    let archive_path = $artifacts | path join $archive

    try {
        mkdir $artifacts

        if $env.ARCHIVE == zip {
            let root = $env.PWD
            cd ($binary | path dirname)
            checked 7z a $archive_path ($binary | path basename)
            cd $root
        } else {
            (checked
                tar
                "-C"
                ($binary | path dirname)
                "-czf"
                $archive_path
                ($binary | path basename)
            )
        }

        let digest = open --raw $archive_path | hash sha256
        ($digest + "  " + $archive + (char newline)) | save --force $"($archive_path).sha256"
    } catch {|error| fail $error.msg }
}

def "main upload" []: nothing -> nothing {
    let archive = $"artifacts/rojo-schema-($env.TARGET).($env.ARCHIVE)"
    (checked
        gh
        release
        upload
        $env.RELEASE_TAG
        $archive
        $"($archive).sha256"
        "--repo"
        $env.REPOSITORY
        "--clobber"
    )
}

def "main publish" []: nothing -> nothing {
    (checked
        gh
        release
        edit
        $env.RELEASE_TAG
        "--repo"
        $env.REPOSITORY
        "--draft=false"
        "--latest"
    )
}

def "main trigger-pages" []: nothing -> nothing {
    (checked
        gh
        workflow
        run
        pages.yml
        "--repo"
        $env.REPOSITORY
        "--ref"
        $env.DEFAULT_BRANCH
    )
}

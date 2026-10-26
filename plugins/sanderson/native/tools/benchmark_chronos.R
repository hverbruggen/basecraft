#!/usr/bin/env Rscript
## Benchmark-only ape 5.8-1 comparison. Never used by the Basecraft plugin.
args <- commandArgs(trailingOnly = TRUE)
if (length(args) != 3) stop("usage: benchmark_chronos.R FIXTURE OUTPUT REPETITIONS")
fixture <- args[[1]]
output <- args[[2]]
repetitions <- as.integer(args[[3]])
dir.create(output, showWarnings = FALSE, recursive = TRUE)
suppressPackageStartupMessages(library(ape))
phy <- read.tree(file.path(fixture, "tree.nwk"))
config <- read.delim(file.path(fixture, "config.tsv"), check.names = FALSE)
lambda <- config$lambda[[1]]
cal <- makeChronosCalib(phy, node = "root", age.min = 1, age.max = 1)
if (config$constraint[[1]] == 1) {
    first.half <- phy$tip.label[seq_len(Ntip(phy) / 2)]
    fixed <- getMRCA(phy, first.half)
    cal <- rbind(cal, makeChronosCalib(phy, node = fixed,
                                     age.min = config$constraint_age[[1]],
                                     age.max = config$constraint_age[[1]]))
}
dual.limit <- as.integer(Sys.getenv("APE_DUAL_ITER_MAX", "20"))
control <- chronos.control(tol = 1e-8, iter.max = 2000, eval.max = 200000,
                           dual.iter.max = dual.limit, epsilon = 1e-6)

write_fit <- function(fit, file) {
    n <- Ntip(fit)
    root <- n + 1L
    children <- split(fit$edge[, 2L], fit$edge[, 1L])
    ages <- rep(NA_real_, n + fit$Nnode)
    ages[[root]] <- 1
    walk_ages <- function(parent) {
        indices <- which(fit$edge[, 1L] == parent)
        for (edge in indices) {
            child <- fit$edge[edge, 2L]
            ages[[child]] <<- ages[[parent]] - fit$edge.length[[edge]]
            if (child > n) walk_ages(child)
        }
    }
    walk_ages(root)
    clade <- function(node) {
        if (node <= n) return(fit$tip.label[[node]])
        child.clades <- unlist(lapply(children[[as.character(node)]], clade))
        tips <- unlist(strsplit(child.clades, ",", fixed = TRUE))
        paste(sort(tips, method = "radix"), collapse = ",")
    }
    rows <- data.frame(clade = vapply(fit$edge[, 2L], clade, character(1)),
                       age = ages[fit$edge[, 2L]],
                       rate = as.numeric(attr(fit, "rates")),
                       edge = seq_len(nrow(fit$edge)))
    write.table(rows, file = file, sep = "\t", row.names = FALSE, quote = FALSE)
}

for (rep in seq_len(repetitions)) {
    set.seed(17)
    warnings <- character()
    start <- proc.time()[["elapsed"]]
    fit <- tryCatch(withCallingHandlers(
        chronos(phy, lambda = lambda, model = "correlated", quiet = TRUE,
                calibration = cal, control = control),
        warning = function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart("muffleWarning")
        }), error = function(e) e)
    elapsed <- proc.time()[["elapsed"]] - start
    meta <- data.frame(elapsed_s = elapsed, convergence = FALSE, ploglik = NA_real_,
                       message = "", warnings = paste(warnings, collapse = "; "),
                       niter = 0L, dual_iter_max = dual.limit)
    if (inherits(fit, "error")) {
        meta$message <- conditionMessage(fit)
    } else {
        meta$convergence <- isTRUE(attr(fit, "convergence"))
        meta$ploglik <- as.numeric(attr(fit, "ploglik"))
        meta$message <- as.character(attr(fit, "message"))
        meta$niter <- as.integer(attr(fit, "niter"))
        write_fit(fit, file.path(output, sprintf("fit-%03d.tsv", rep)))
    }
    meta$message <- gsub("[\t\r\n]", " ", meta$message)
    meta$warnings <- gsub("[\t\r\n]", " ", meta$warnings)
    write.table(meta, file.path(output, sprintf("meta-%03d.tsv", rep)), sep = "\t",
                row.names = FALSE, quote = FALSE)
}

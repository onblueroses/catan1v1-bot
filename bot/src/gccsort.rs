//! The libstdc++ (GCC 13) `std::sort` order (BOT-SPEC 5.5). It is not stable,
//! and the order of equal keys is part of the bot's behaviour.

const THRESHOLD: usize = 16;

/// Sort `v` so that "a before b" holds for `before(a, b)`, reproducing the
/// exact permutation of GCC 13 `std::sort`.
pub fn gcc_sort<T: Copy>(v: &mut [T], before: impl Fn(&T, &T) -> bool) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let depth = 2 * (usize::BITS - 1 - n.leading_zeros()) as usize;
    introsort_loop(v, 0, n, depth, &before);
    // Final insertion sort: equivalent to the guarded/unguarded pair.
    for i in 1..n {
        let val = v[i];
        let mut j = i;
        while j > 0 && before(&val, &v[j - 1]) {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = val;
    }
}

fn introsort_loop<T: Copy>(v: &mut [T], first: usize, mut last: usize, mut depth: usize, before: &impl Fn(&T, &T) -> bool) {
    while last - first > THRESHOLD {
        if depth == 0 {
            heap_sort(&mut v[first..last], before);
            return;
        }
        depth -= 1;
        let cut = partition_pivot(v, first, last, before);
        introsort_loop(v, cut, last, depth, before);
        last = cut;
    }
}

fn partition_pivot<T: Copy>(v: &mut [T], first: usize, last: usize, before: &impl Fn(&T, &T) -> bool) -> usize {
    let mid = first + (last - first) / 2;
    let (a, b, c) = (first + 1, mid, last - 1);
    let pick = if before(&v[a], &v[b]) {
        if before(&v[b], &v[c]) {
            b
        } else if before(&v[a], &v[c]) {
            c
        } else {
            a
        }
    } else if before(&v[a], &v[c]) {
        a
    } else if before(&v[b], &v[c]) {
        c
    } else {
        b
    };
    v.swap(first, pick);
    // Unguarded partition around the pivot at `first`.
    let mut i = first + 1;
    let mut j = last;
    loop {
        while before(&v[i], &v[first]) {
            i += 1;
        }
        j -= 1;
        while before(&v[first], &v[j]) {
            j -= 1;
        }
        if i >= j {
            return i;
        }
        v.swap(i, j);
        i += 1;
    }
}

fn adjust_heap<T: Copy>(v: &mut [T], hole0: usize, len: usize, value: T, before: &impl Fn(&T, &T) -> bool) {
    let top = hole0;
    let mut hole = hole0;
    let mut child = hole0;
    while len >= 1 && child < (len - 1) / 2 {
        child = 2 * (child + 1);
        if before(&v[child], &v[child - 1]) {
            child -= 1;
        }
        v[hole] = v[child];
        hole = child;
    }
    if len % 2 == 0 && len >= 2 && child == (len - 2) / 2 {
        child = 2 * (child + 1);
        v[hole] = v[child - 1];
        hole = child - 1;
    }
    // Push up.
    while hole > top {
        let parent = (hole - 1) / 2;
        if !before(&v[parent], &value) {
            break;
        }
        v[hole] = v[parent];
        hole = parent;
    }
    v[hole] = value;
}

fn heap_sort<T: Copy>(v: &mut [T], before: &impl Fn(&T, &T) -> bool) {
    let m = v.len();
    if m < 2 {
        return;
    }
    let mut parent = (m - 2) / 2;
    loop {
        let value = v[parent];
        adjust_heap(v, parent, m, value, before);
        if parent == 0 {
            break;
        }
        parent -= 1;
    }
    let mut end = m;
    while end > 1 {
        end -= 1;
        let value = v[end];
        v[end] = v[0];
        adjust_heap(v, 0, end, value, before);
    }
}

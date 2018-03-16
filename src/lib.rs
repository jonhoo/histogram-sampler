extern crate rand;

use std::collections::BTreeMap;
use std::collections::Bound;

pub struct Sampler {
    bins: BTreeMap<usize, (usize, usize)>,
    end: usize,
}

impl Sampler {
    pub fn from_bins<I>(iter: I, bin_width: usize) -> Self
    where
        I: IntoIterator<Item = (usize, usize)>,
    {
        let mut start = 0;
        let mut next_id = 0;
        let mut bins = BTreeMap::default();

        for (bin, count) in iter {
            // we want the likelihood of selecting an id in this bin to be proportional to
            // average bin value * `count`. the way to think about that in the context of sampling
            // from a histogram is that there are `count` ranges, each spanning an interval of
            // width `bin`. we can improve on this slightly by just keeping track of a single
            // interval of width average bin value * count, and then convert the chosen value into
            // an id by doing a % count.
            bins.insert(start, (next_id, count));

            // the bucket *centers* on bin, so it captures everything within bin_width/2 on either
            // side. in general, the average bin value should therefore just be the bin value. the
            // exception is the very first bin, which only holds things in [0, bin_width/2), since
            // everything above that would be rounded to the *next* bin. so, for things in the very
            // first bin, the average value is really bin_width/4. to avoid fractions, we instead
            // oversample by a factor of 4.
            let avg_bin_value = if bin == 0 { bin_width } else { 4 * bin };

            start += count * avg_bin_value;
            next_id += count;
        }

        Sampler {
            bins: bins,
            end: start,
        }
    }
}

impl rand::distributions::Sample<usize> for Sampler {
    fn sample<R: rand::Rng>(&mut self, rng: &mut R) -> usize {
        use rand::distributions::IndependentSample;
        self.ind_sample(rng)
    }
}

impl rand::distributions::IndependentSample<usize> for Sampler {
    fn ind_sample<R: rand::Rng>(&self, rng: &mut R) -> usize {
        let sample = rng.gen_range(0, self.end);

        // find the bin we're sampling from
        let &(first_id, n) = self.bins
            .range((Bound::Unbounded, Bound::Included(sample)))
            .next_back()
            .unwrap()
            .1;

        // find a value in the bin's range
        first_id + (sample % n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::AddAssign;
    use std::collections::HashMap;
    use rand::distributions::IndependentSample;

    #[test]
    fn it_works() {
        let stories_per_votecount = vec![
            (0, 16724),
            (10, 16393),
            (20, 4601),
            (30, 1707),
            (40, 680),
            (50, 281),
            (60, 128),
            (70, 60),
            (80, 35),
            (90, 16),
            (100, 4),
            (110, 4),
            (120, 10),
            (130, 1),
            (140, 2),
            (160, 1),
            (210, 1),
            (250, 1),
            (290, 1),
        ];

        // compute stats over the original data so we can compare the generated stream
        let data_nstories: isize = stories_per_votecount.iter().map(|&(_, n)| n as isize).sum();
        let data_nvotes = stories_per_votecount
            .iter()
            .map(|&(bin, n)| (bin * n) as isize)
            .sum::<isize>()
            + (stories_per_votecount.iter().next().unwrap().1 as f64 * 0.25) as isize;

        // what proportion of stories are in each bin?
        let data_proportions: Vec<_> = stories_per_votecount
            .iter()
            .map(|&(bin, n)| (bin, (n as f64 / data_nstories as f64)))
            .collect();

        // make our sampler, and sample from it, keeping track of the resulting vote counts note
        // that we must sample the same number of votes as are in the original dataset to expect
        // the resulting histogram to be the same (if we sample more, the bins will shift towards
        // higher values).
        let mut rng = rand::thread_rng();
        let mut votes = HashMap::new();
        let vote_sampler = Sampler::from_bins(stories_per_votecount, 10);
        for _ in 0..data_nvotes {
            votes
                .entry(vote_sampler.ind_sample(&mut rng))
                .or_insert(0)
                .add_assign(1);
        }

        // compute the histogram of the sampled dataset
        let mut hist = HashMap::new();
        for (_, votes) in votes {
            hist.entry(10 * ((votes + 5) / 10))
                .or_insert(0)
                .add_assign(1);
        }

        // compute the same statistics over the sampled data
        let nstories: isize = hist.iter().map(|(_, &n)| n as isize).sum();
        let nvotes = hist.iter().map(|(&bin, &n)| bin * n).sum::<isize>()
            + (hist[&0] as f64 * 0.25) as isize;

        // number of stories and votes should be roughly (< 5%) the same
        println!("story count: {} -> {}", data_nstories, nstories);
        println!("vote count: {} -> {}", data_nvotes, nvotes);
        assert!((data_nstories - nstories).abs() < data_nstories / 20);
        assert!((data_nvotes - nvotes).abs() < data_nvotes / 20);

        // to compare the histograms we need to walk the rows of both
        let mut expected_props = data_proportions.iter().peekable();

        // let's do it in numerical order so we get more readable output
        let mut keys: Vec<_> = hist.keys().cloned().collect();
        keys.sort();

        for &bin in &keys {
            // proportion of stories in this bin
            let prop = hist[&bin] as f64 / nstories as f64;

            // get the proportion of stories in the same bin in the original histogram
            if let Some(&&(exp_bin, exp_prop)) = expected_props.peek() {
                // make sure we keep moving through the original histogram
                if exp_bin <= bin as usize {
                    expected_props.next();
                }

                if exp_bin != bin as usize {
                    // this better be a small bin if it doesn't match the original
                    assert!(prop < 0.005);
                    println!(
                        "{}\t{:>4.1}%\t??\t{}\t{:>4.1}%",
                        exp_bin,
                        100.0 * exp_prop,
                        bin,
                        100.0 * prop,
                    );
                    continue;
                }

                // how well did we do?
                let diff = prop - exp_prop;

                println!(
                    "{}\t{:>4.1}%\t->\t{}\t{:>4.1}%\t(diff: {:>5.2})",
                    exp_bin,
                    100.0 * exp_prop,
                    bin,
                    100.0 * prop,
                    100.0 * diff
                );

                if prop > 0.005 {
                    // any bucket with .5% or more stories shoud match pretty well
                    // the exception is the first and second bucket
                    if bin == keys[0] {
                        // it's really hard to sample accurately near 0 with a bucket width of 10,
                        // since the chance of accidentally spilling over to >=5 is so high.
                        // so, we tolerate a larger (negative) error in this case
                        //
                        // NOTE: if we double the widths of the bins, this artefact goes away
                        assert!(diff < 0.0 && diff > -0.05);
                    } else if bin == keys[1] {
                        // things that spill over from bin 0 spill into bin 1
                        assert!(diff > 0.0 && diff < 0.05);
                    } else {
                        // all other buckets we should have very small errors (< .7pp)
                        assert!(diff.abs() < 0.007);
                    }
                }
            } else {
                println!("\t\t??\t{}\t{:>4.1}%", bin, 100.0 * prop);
                // bins better be rare for them to differ from original histogram
                assert!(prop < 0.01);
            }
        }
    }
}

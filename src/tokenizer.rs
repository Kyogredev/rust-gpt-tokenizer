use pcre2::bytes::{RegexBuilder, Regex as Pcre2Regex};
use std::{collections::HashMap, error::Error};
use std::{hash::Hash, cmp::Eq};
use itertools::Itertools;
use regex::Regex;

#[derive(Debug, Clone)]
pub struct GPT3_Tokenizer {
  bpe_regexp: Pcre2Regex,
  encodings: HashMap<String, u32>,
  decodings: HashMap<u32, String>,
  byte_encoder: HashMap<u8, char>,
  byte_decoder: HashMap<char, u8>,
  bpe_ranks: HashMap<(String, String), u32>
}

impl GPT3_Tokenizer {

  pub fn new() -> Self {
    // The raw regex string has been modified from the original implementation for two compatibility
    // issues between javascript's regex engine and pcre2, which is the engine used here.
    // The first issue concerns the way unicode points are represented:
    // https://www.regular-expressions.info/unicode.html#:~:text=Matching%20a%20Specific%20Code%20Point
    // The second issue concerns the impossibility for pcre2 to compile non unicode scalar values:
    // https://www.unicode.org/glossary/#unicode_scalar_value
    // Both compilation issues have been solved with seemingly minimal end-result inconsistency.
    let raw_regex = include_str!("./lib/GPT3regexp.txt");
    let bpe_vocab = include_str!("./lib/GPT3vocab.txt");
    let raw_encodings = include_str!("./lib/GPT3encodings.json");

    let bpe_regexp = RegexBuilder::new()
    .utf(true)
    .ucp(true)
    .build(&raw_regex)
    .expect("Could not compile Regex.");

    let merges = Self::get_merges(&bpe_vocab);  

    let encodings = Self::get_encodings(&raw_encodings);
    let decodings = swap_hashmap(encodings.clone());
    
    let byte_encoder = Self::get_byte_encoder();
    let byte_decoder = swap_hashmap(byte_encoder.clone());

    let bpe_ranks = Self::get_bpe_ranks(merges.clone()).unwrap();

    GPT3_Tokenizer {
      bpe_regexp,
      encodings,
      decodings,
      byte_encoder,
      byte_decoder,
      bpe_ranks
    }
  }

  // Below are helper methods called solely during construction, performance is not
  // considered an issue for these and they can (and should) panic when errors occur.

  fn get_merges(bpe_vocab: &str) -> Vec<(String, String)> {
    let re = Regex::new(r"(\s+)").expect("Could not compile Regex.");

    let unicoded = raw_to_unicode(bpe_vocab);

    let b = &unicoded[1..unicoded.len() - 1];
    let c: Vec<(String, String)> = b.iter()
    .map(|each| {
      let pair: (String, String) = re.split(each)
      .map(|mat| mat.to_string()).collect_tuple().unwrap();
      pair
    }).collect();
    c
  }

  fn get_encodings(encodings: &str)
  -> HashMap<String, u32>
  {
    let enc: HashMap<String, u32> = serde_json::from_str(encodings)
    .expect("Could not parse encodings hashmap from json.");
    enc
  }

  fn get_bpe_ranks(merges: Vec<(String, String)>)
  -> Result<HashMap<(String, String), u32>, Box<dyn Error>>
  {
    let a = range(0, merges.len().try_into()?);
    let ranks = merges.iter().zip(a.iter()).map(|(merge, n)| {
      (merge.to_owned(), n.to_owned())
    }).collect();
    Ok(ranks)
  }

  fn get_byte_encoder() -> HashMap<u8, char> {
    let a = range('!' as u32, ('~' as u32) + 1);
    let b = range('¡' as u32, ('¬' as u32) + 1);
    let c = range('®' as u32, ('ÿ' as u32) + 1);
    let mut bs = [[a, b].concat(), c].concat();
    
    let mut cs = bs.clone();
    let mut n = 0;

    for b in 0..256 {
      if !bs.contains(&b) {
          bs.push(b);
          cs.push(256 + n);
          n = n + 1;
      }
    };

    let cs_char: Vec<char> = cs.iter()
    .filter_map(|&each| char::from_u32(each))
    .collect();

    let result: HashMap<u8, char> = bs.iter()
    .zip(cs_char.iter())
    .map(|(&eachbs, eachcs)| {
      (u8::try_from(eachbs).unwrap(), eachcs.to_owned())
    }).collect();

    result
  }

  // Below are methods called for every invokation of encode().

  /// Takes a vector of single characters `["a", "b", "c", "d", "e"]` and returns a vector of pairs `["ab", "bc", "cd", "de"]`
  fn get_pairs(word: &[String]) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut prev = word[0].clone();
    for i in 1..word.len() {
      let char = &word[i];
      pairs.push((prev, char.clone()));
      prev = char.clone();
    }
    pairs
  }

  fn bpe(&self, token: &str) -> Result<String, Box<dyn Error>> {
    // check cache here
    
    let wrd: Vec<&str> = token.split("").collect();

    // this is needed because split("") in Rust always returns 
    // two empty "" at the beginning and end of the resulting iterator.
    let mut word: Vec<String> = wrd[1..wrd.len() - 1].iter().map(|z| z.to_string()).collect();

    let mut pairs = Self::get_pairs(&word);

    if pairs.len() == 0 {
      return Ok(token.to_string())
    }

    loop {
      let mut min_pairs: HashMap<u32, &(String, String)> = HashMap::new();   //1 - THIS NOT NEED BE AN HASHMAP, BECAUSE KEYS MUST BE NON-UNIQUE
      for pair in pairs.iter() {
        let rank = self.bpe_ranks.get(pair);
        min_pairs.insert(rank.unwrap_or(&u32::MAX).to_owned(), pair);  
      }

      // println!("PAIRS HAVE THIS RANK: {:?}", min_pairs);

      let min_key = min_pairs.keys().min().unwrap();  // we can safely unwrap here, as min_pairs is non-empty
      let bigram = min_pairs.get(min_key).unwrap().to_owned();

      // println!("LOWEST PAIR IS: {:?}", bigram);

      if !self.bpe_ranks.contains_key(bigram) {
        break
      }

      let first = bigram.to_owned().0;
      let second = bigram.to_owned().1;
      let mut newWord: Vec<String> = Vec::new();
      let mut i = 0;

      while i < word.len() {
        let mut j = word.iter()
        .skip(i)
        .position(|x| x == &first)
        .unwrap_or(usize::MAX);     // position() returns usize 
        if j == usize::MAX {
          newWord.extend_from_slice(&word[i..word.len()]);
          break
        } else {
          j = j + i       // this is needed to match the behaviour of indexOf() in js
        }

        newWord.extend_from_slice(&word[i..j]); 
        i = j;

        if word[i] == first && i < (word.len() - 1) && word[i + 1] == second {
          newWord.push(first.clone() + &second);
          i = i + 2;
        } else {
          newWord.push(word[i].clone());
          i = i + 1;
        }
      }

      word = newWord;

      if word.len() == 1 {
        // println!("REACHED BREAKING POINT ON WORD: {:?}", word);
        break
      } else {
        pairs = Self::get_pairs(&word)
      }
    }

    let output = word.join(" ");
    // cache word here
    Ok(output)
  }
 
  fn decode(&self, value: &u32) -> Option<String> {
    let text = self.decodings.get(value)?.clone();
    let chars: Vec<char> = text.chars().collect();
    let utf8: Vec<u8> = chars.iter()
    .filter_map(|x| self.byte_decoder.get(x))
    .copied().collect();
    let decoded = String::from_utf8(utf8).ok()?;
    Some(decoded)
  }

  pub fn encode(&self, text: &str)
  -> Result<(usize, Vec<u32>, Vec<String>), Box<dyn Error>> {
    let mut bpeTokens = Vec::new();
    let mut texts: Vec<String> = Vec::new();

    let mut matches: Vec<String> = Vec::new();
    for result in self.bpe_regexp.find_iter(text.as_bytes()) {
      let mat = result?;
      let slice = &text[mat.start()..mat.end()];
      matches.push(slice.to_string());
    }

    for token in matches.iter() {
        let x: String = token.as_bytes().iter()        
        .filter_map(|byte| self.byte_encoder.get(byte))
        .collect();

        let newTokens: Vec<u32> = Self::bpe(&self, &x)?.split(" ")
        .filter_map(|each| self.encodings.get(each))
        .copied().collect();

        bpeTokens.extend_from_slice(&newTokens);

        let to_append: Vec<String> = newTokens.iter()
        .filter_map(|each| Self::decode(&self, each)) 
        .collect();
        texts.extend(to_append);
    };

    Ok((bpeTokens.len(), bpeTokens, texts))
  }
}

/// Returns a *new* hashmap where key-value pairs are swapped, consuming the original.
fn swap_hashmap<K, V: Hash + Eq>(hashmap: HashMap<K, V>) -> HashMap<V, K> {
  let res = hashmap.into_iter().map(|(k, v)| (v, k)).collect();
  res
}
 
/// Generates a vector of numbers starting at `start` and ending at `end - 1`
fn range(start: u32, end: u32) -> Vec<u32> {
  (start..end).collect()
}
/// Converts a raw string formally encoding a unicode to an actual unicode char. Panics on failure.
/// ```
/// assert_eq!("Ċ".to_string(), raw_to_unicode_char(r"\u{010a}").to_string());
/// ```
/// https://stackoverflow.com/questions/40055279
fn raw_to_unicode_char(s: &str) -> char {
  let number = &s[3..7];                                    // takes the portion "wxyz" of "\u{wxyz}"
  let hexa = u32::from_str_radix(number, 16).unwrap();      
  std::char::from_u32(hexa).unwrap()
}

/// Does the same as raw_to_unicode_char, but for ASCII sequences
fn raw_to_ascii_char(s: &str) -> char {
  let number = &s[2..];                                     // takes the portion "00" of "\x00"
  let hexa = u32::from_str_radix(number, 16).unwrap();
  std::char::from_u32(hexa).unwrap()
}

/// Transforms provided raw string vocabulary to properly UTF-8 encoded strings.
fn raw_to_unicode(vocab: &str) -> Vec<String> {
  let unicode = Regex::new(r"\\u\{[0-9a-fA-F]{4}\}").expect("Could not compile Regex");  //finds "\u{0000}"
  let ascii = Regex::new(r"\\x[0-9a-fA-F]{2}").expect("Could not compile Regex");        //finds "\x00"

  let new_lined = vocab.replace("\\n", "\n");
  let line_splits: Vec<&str> = new_lined.split("\n").collect();
  
  let mut out: Vec<String> = Vec::new();

  for line in line_splits {
    let mut new = line.to_owned();

    if line.contains("\\u{") {       // deals with unicode points
      let mut loc = unicode.find(&new);   
      while let Some(mat) = loc {
        new.replace_range(mat.start()..mat.end(), &raw_to_unicode_char(mat.as_str()).to_string());
        loc = unicode.find(&new);        
      }
    } 
    
    if line.contains("\\x") {        // deals with ASCII
      let mut loc = ascii.find(&new);
      while let Some(mat) = loc {
        new.replace_range(mat.start()..mat.end(), &raw_to_ascii_char(mat.as_str()).to_string());
        loc = ascii.find(&new);
      }
    } 

    out.push(new)
  }
  out
}
// #![deny(warnings)]

#![warn(unused_extern_crates)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::panic)]
#![deny(clippy::unreachable)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::needless_pass_by_value)]
#![deny(clippy::trivially_copy_pass_by_ref)]
// We allow expect since it forces good error messages at the least.
#![allow(clippy::expect_used)]

#[macro_use]
extern crate tracing;

pub mod filter;
pub mod proto;
pub mod simple;

use bytes::{Buf, BytesMut};
use lber::parse::Parser;
use lber::structure::StructureTag;
use lber::write as lber_write;
use lber::{Consumer, ConsumerState, Input, Move};
use std::convert::TryFrom;
use std::io;
use tokio_util::codec::{Decoder, Encoder};

pub use crate::filter::parse_ldap_filter_str;
use crate::proto::LdapMsg;
pub use crate::simple::*;

pub struct LdapCodec;

impl Decoder for LdapCodec {
    type Item = LdapMsg;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // How many bytes to consume?
        let mut parser = Parser::new();
        let (size, msg) = match *parser.handle(Input::Element(buf)) {
            ConsumerState::Continue(_) => return Ok(None),
            ConsumerState::Error(_e) => {
                return Err(io::Error::new(io::ErrorKind::Other, "lber parser"))
            }
            ConsumerState::Done(size, ref msg) => (size, msg),
        };
        // Consume that
        let size = match size {
            Move::Await(_) => return Ok(None),
            Move::Seek(_) => return Err(io::Error::new(io::ErrorKind::Other, "lber seek")),
            Move::Consume(s) => s,
        };
        // helper for when we need to debug inputs.
        trace!("{:?}", buf.to_vec());
        if size == buf.len() {
            buf.clear();
        } else {
            buf.advance(size);
        }
        // Build the LdapMsg from the Tag
        LdapMsg::try_from(msg.clone())
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "ldapmsg invalid"))
            .map(Some)
    }
}

impl Encoder<LdapMsg> for LdapCodec {
    // type Item = LdapMsg;
    type Error = io::Error;

    fn encode(&mut self, msg: LdapMsg, buf: &mut BytesMut) -> io::Result<()> {
        let encoded: StructureTag = msg.into();
        lber_write::encode_into(buf, encoded)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::proto::*;
    use crate::LdapCodec;
    use bytes::BytesMut;
    use std::convert::TryInto;
    use tokio_util::codec::{Decoder, Encoder};

    macro_rules! do_test {
        ($req:expr) => {{
            let _ = tracing_subscriber::fmt::try_init();
            let mut buf = BytesMut::new();
            let mut server_codec = LdapCodec;
            assert!(server_codec.encode($req.clone(), &mut buf).is_ok());
            debug!("buf {:x}", buf);
            let res = server_codec.decode(&mut buf).expect("failed to decode");
            let msg = res.expect("None found?");
            debug!("{:?}", msg);
            assert!($req == msg)
        }};
    }

    #[test]
    fn test_ldapserver_codec_simplebind() {
        do_test!(LdapMsg {
            msgid: 1,
            op: LdapOp::BindRequest(LdapBindRequest {
                dn: "".to_string(),
                cred: LdapBindCred::Simple("".to_string()),
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_unbind() {
        do_test!(LdapMsg {
            msgid: 65536,
            op: LdapOp::UnbindRequest,
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_bindresponse() {
        do_test!(LdapMsg {
            msgid: 999999,
            op: LdapOp::BindResponse(LdapBindResponse {
                res: LdapResult {
                    code: LdapResultCode::Success,
                    matcheddn: "cn=Directory Manager".to_string(),
                    message: "It works!".to_string(),
                    referral: vec![],
                },
                saslcreds: None
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_searchrequest() {
        do_test!(LdapMsg {
            msgid: 2_147_483_646,
            op: LdapOp::SearchRequest(LdapSearchRequest {
                base: "dc=example,dc=comaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                scope: LdapSearchScope::Base,
                aliases: LdapDerefAliases::Never,
                sizelimit: 0,
                timelimit: 0,
                typesonly: false,
                filter: LdapFilter::Or(vec![
                    LdapFilter::Present("cn".to_string()),
                    LdapFilter::Equality("cn".to_string(), "name".to_string()),
                    LdapFilter::Not(Box::new(LdapFilter::And(vec![LdapFilter::Present(
                        "cursed".to_string()
                    ),]))),
                    LdapFilter::Substring(
                        "cn".to_string(),
                        LdapSubstringFilter {
                            initial: Some("abc".to_string()),
                            any: vec!["def".to_string(), "ghi".to_string()],
                            final_: Some("jkl".to_string())
                        }
                    ),
                    LdapFilter::Substring(
                        "cn".to_string(),
                        LdapSubstringFilter {
                            initial: None,
                            any: vec![],
                            final_: None
                        }
                    )
                ]),
                attrs: vec!["cn".to_string(), "objectClass".to_string(),],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_searchresultentry() {
        do_test!(LdapMsg {
            msgid: 2_147_483_646,
            op: LdapOp::SearchResultEntry(LdapSearchResultEntry {
                dn: "cn=demo,dc=example,dc=com".to_string(),
                attributes: vec![
                    LdapPartialAttribute {
                        atype: "cn".to_string(),
                        vals: vec!["demo".as_bytes().to_vec(),]
                    },
                    LdapPartialAttribute {
                        atype: "dn".to_string(),
                        vals: vec!["cn=demo,dc=example,dc=com".as_bytes().to_vec(),]
                    },
                    LdapPartialAttribute {
                        atype: "objectClass".to_string(),
                        vals: vec!["cursed".as_bytes().to_vec(),]
                    },
                ]
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_searchresultdone() {
        do_test!(LdapMsg {
            msgid: 28799790,
            op: LdapOp::SearchResultDone(LdapResult {
                code: LdapResultCode::Success,
                matcheddn: "".to_string(),
                message: "Whargarble".to_string(),
                referral: vec![],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_extendedrequest() {
        do_test!(LdapMsg {
            msgid: 256,
            op: LdapOp::ExtendedRequest(LdapExtendedRequest {
                name: "1.3.6.1.4.1.4203.1.11.3".to_string(),
                value: None,
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_extendedresponse() {
        do_test!(LdapMsg {
            msgid: 257,
            op: LdapOp::ExtendedResponse(LdapExtendedResponse {
                res: LdapResult {
                    code: LdapResultCode::Success,
                    matcheddn: "".to_string(),
                    message: "".to_string(),
                    referral: vec![],
                },
                name: Some("1.3.6.1.4.1.4203.1.11.3".to_string()),
                value: None,
            }),
            ctrl: vec![],
        });

        do_test!(LdapMsg {
            msgid: 257,
            op: LdapOp::ExtendedResponse(LdapExtendedResponse {
                res: LdapResult {
                    code: LdapResultCode::Success,
                    matcheddn: "".to_string(),
                    message: "".to_string(),
                    referral: vec![],
                },
                name: None,
                value: Some(Vec::from("hello")),
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_addrequest() {
        do_test!(LdapMsg {
            msgid: 233,
            op: LdapOp::AddRequest(LdapAddRequest {
                dn: "dc=example,dc=com".to_string(),
                attributes: vec![LdapPartialAttribute {
                    atype: "objectClass".to_string(),
                    vals: vec![
                        "top".as_bytes().to_vec(),
                        "posixAccount".as_bytes().to_vec()
                    ]
                }],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_addresponse() {
        do_test!(LdapMsg {
            msgid: 23333,
            op: LdapOp::AddResponse(LdapResult {
                code: LdapResultCode::Success,
                matcheddn: "dc=exmaple,dc=com".to_string(),
                message: "msg".to_string(),
                referral: vec![],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_delrequest() {
        do_test!(LdapMsg {
            msgid: 233,
            op: LdapOp::DelRequest("dc=example, dc=com".to_string()),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_delresponse() {
        do_test!(LdapMsg {
            msgid: 23333,
            op: LdapOp::DelResponse(LdapResult {
                code: LdapResultCode::Success,
                matcheddn: "dc=exmaple,dc=com".to_string(),
                message: "msg".to_string(),
                referral: vec![],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_abandonrequest() {
        do_test!(LdapMsg {
            msgid: 23333,
            op: LdapOp::AbandonRequest(233),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_modify_request() {
        do_test!(LdapMsg {
            msgid: 1,
            op: LdapOp::ModifyRequest(LdapModifyRequest {
                dn: "cn=bob,ou=people,dc=example,dc=com".to_string(),
                changes: vec![LdapModify {
                    operation: LdapModifyType::Replace,
                    modification: LdapPartialAttribute {
                        atype: "userPassword".to_string(),
                        vals: vec!["password".as_bytes().to_vec()],
                    }
                }],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_ldapserver_codec_modify_response() {
        do_test!(LdapMsg {
            msgid: 1,
            op: LdapOp::ModifyResponse(LdapResult {
                code: LdapResultCode::Success,
                matcheddn: "cn=Directory Manager".to_string(),
                message: "It works!".to_string(),
                referral: vec![],
            }),
            ctrl: vec![],
        });
    }

    #[test]
    fn test_modify_from_raw() {
        use lber::Consumer;
        use std::convert::TryFrom;

        let mut parser = lber::parse::Parser::new();
        let (_size, msg) = match *parser.handle(lber::Input::Element(&[
            48, 69, 2, 1, 2, 102, 64, 4, 39, 117, 105, 100, 61, 98, 106, 101, 110, 115, 101, 110,
            44, 111, 117, 61, 80, 101, 111, 112, 108, 101, 44, 100, 99, 61, 101, 120, 97, 109, 112,
            108, 101, 44, 100, 99, 61, 99, 111, 109, 48, 21, 48, 19, 10, 1, 2, 48, 14, 4, 2, 115,
            110, 49, 8, 4, 6, 77, 111, 114, 114, 105, 115,
        ])) {
            lber::ConsumerState::Done(size, ref msg) => (size, msg),
            _ => panic!(),
        };
        let op = LdapMsg::try_from(msg.clone()).expect("failed to decode");

        eprintln!("{:?}", op);
    }

    #[test]
    fn test_syncrepl_result_from_raw() {
        use lber::Consumer;
        use std::convert::TryFrom;

        let _ = tracing_subscriber::fmt::try_init();

        let mut parser = lber::parse::Parser::new();
        let (_size, msg) = match *parser.handle(lber::Input::Element(&[
            48, 35, 2, 1, 2, 101, 30, 10, 2, 16, 0, 4, 0, 4, 22, 73, 110, 118, 97, 108, 105, 100,
            32, 115, 101, 115, 115, 105, 111, 110, 32, 99, 111, 111, 107, 105, 101,
        ])) {
            lber::ConsumerState::Done(size, ref msg) => (size, msg),
            _ => panic!(),
        };
        let op = LdapMsg::try_from(msg.clone()).expect("failed to decode");

        eprintln!("{:?}", op);
    }

    #[test]
    fn test_ldapserver_password_extop() {
        let mrq = LdapPasswordModifyRequest {
            user_identity: Some("william".to_string()),
            old_password: Some("abcd".to_string()),
            new_password: Some("dcba".to_string()),
        };

        let ler: LdapExtendedRequest = mrq.clone().into();
        let mrq_dec: LdapPasswordModifyRequest = (&ler).try_into().unwrap();
        assert!(mrq == mrq_dec);

        let mrs = LdapPasswordModifyResponse {
            res: LdapResult {
                code: LdapResultCode::Success,
                matcheddn: "uid=william,dc=exmaple,dc=com".to_string(),
                message: "msg".to_string(),
                referral: vec![],
            },
            gen_password: Some("abcd".to_string()),
        };

        let ler: LdapExtendedResponse = mrs.clone().into();
        let mrs_dec: LdapPasswordModifyResponse = (&ler).try_into().unwrap();
        assert!(mrs == mrs_dec);
    }

    #[test]
    fn test_ldapserver_search_with_syncrepl_request() {
        // openldap
        // ctrl_tag=Some(StructureTag { class: Universal, id: 16, payload: C(
        // inner=[StructureTag { class: Context, id: 0, payload: C([StructureTag { class: Universal, id: 4, payload: P([49, 46, 51, 46, 54, 46, 49, 46, 52, 46, 49, 46, 52, 50, 48, 51, 46, 49, 46, 57, 46, 49, 46, 49]) }, StructureTag { class: Universal, id: 1, payload: P([0]) }, StructureTag { class: Universal, id: 4, payload: P([48, 3, 10, 1, 1]) }]) }]) })

        // inner=[StructureTag { class: Universal, id: 16, payload: C([StructureTag { class: Universal, id: 4, payload: P([49, 46, 51, 46, 54, 46, 49, 46, 52, 46, 49, 46, 52, 50, 48, 51, 46, 49, 46, 57, 46, 49, 46, 49]) }, StructureTag { class: Universal, id: 4, payload: P([48, 8, 10, 1, 1, 4, 3, 102, 111, 111]) }]) }]

        do_test!(LdapMsg {
            msgid: 1,
            op: LdapOp::SearchRequest(LdapSearchRequest {
                base: "dc=example,dc=com".to_string(),
                scope: LdapSearchScope::Subtree,
                aliases: LdapDerefAliases::Never,
                sizelimit: 0,
                timelimit: 0,
                typesonly: false,
                filter: LdapFilter::Present("objectClass".to_string()),
                attrs: vec![],
            }),
            ctrl: vec![LdapControl::SyncRequest {
                criticality: false,
                mode: SyncRequestMode::RefreshOnly,
                cookie: None,
                reload_hint: false
            }],
        });
    }
}

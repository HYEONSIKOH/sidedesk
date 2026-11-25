use sidedesk_core::{Hello, encode};

use crate::port::{ComputerName, PhoneLinks, PortError};

#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub target: String,
    pub result: Result<(), PortError>,
}

/// 컴퓨터 이름을 붙어 있는 폰 모두에 보낸다. 한 곳이 실패해도 나머지는 보낸다.
pub fn greet(
    name: &dyn ComputerName,
    links: &[&dyn PhoneLinks],
) -> Result<Vec<Outcome>, PortError> {
    let computer_name = name.read()?;
    if computer_name.trim().is_empty() {
        return Err(PortError::new("컴퓨터 이름이 비어 있음"));
    }
    let bytes = encode(&Hello { computer_name });
    let mut outcomes = Vec::new();
    for link in links {
        match link.attached() {
            Ok(phones) => outcomes.extend(phones.into_iter().map(|phone| Outcome {
                result: match phone.blocked {
                    Some(reason) => Err(PortError(reason)),
                    None => link.send(&phone.id, &bytes),
                },
                target: phone.id,
            })),
            Err(error) => outcomes.push(Outcome {
                target: link.label().to_owned(),
                result: Err(error),
            }),
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::port::Phone;

    struct Name(Result<&'static str, &'static str>);

    impl ComputerName for Name {
        fn read(&self) -> Result<String, PortError> {
            self.0.map(str::to_owned).map_err(PortError::new)
        }
    }

    struct Links {
        attached: Result<Vec<Phone>, PortError>,
        refuses: &'static str,
        sent: RefCell<Vec<(String, Vec<u8>)>>,
    }

    impl Links {
        fn new(attached: Result<Vec<Phone>, PortError>) -> Self {
            Self {
                attached,
                refuses: "",
                sent: RefCell::default(),
            }
        }
    }

    impl PhoneLinks for Links {
        fn label(&self) -> &str {
            "fake"
        }

        fn attached(&self) -> Result<Vec<Phone>, PortError> {
            self.attached.clone()
        }

        fn send(&self, id: &str, bytes: &[u8]) -> Result<(), PortError> {
            if id == self.refuses {
                return Err(PortError::new("거부"));
            }
            self.sent.borrow_mut().push((id.to_owned(), bytes.to_vec()));
            Ok(())
        }
    }

    fn ready(id: &str) -> Phone {
        Phone {
            id: id.to_owned(),
            blocked: None,
        }
    }

    #[test]
    fn sends_the_name_to_every_phone() {
        let links = Links::new(Ok(vec![ready("a"), ready("b")]));
        let outcomes = greet(&Name(Ok("mac")), &[&links]).unwrap();
        assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
        let expected = encode(&Hello {
            computer_name: "mac".to_owned(),
        });
        let sent = links.sent.borrow();
        assert_eq!(sent.len(), 2);
        assert!(sent.iter().all(|(_, bytes)| *bytes == expected));
    }

    #[test]
    fn no_phone_is_an_empty_result() {
        let links = Links::new(Ok(Vec::new()));
        assert_eq!(greet(&Name(Ok("mac")), &[&links]), Ok(Vec::new()));
    }

    #[test]
    fn one_failure_does_not_stop_the_rest() {
        let mut links = Links::new(Ok(vec![ready("a"), ready("b")]));
        links.refuses = "a";
        let outcomes = greet(&Name(Ok("mac")), &[&links]).unwrap();
        assert_eq!(outcomes[0].result, Err(PortError::new("거부")));
        assert_eq!(outcomes[1].result, Ok(()));
    }

    #[test]
    fn a_blocked_phone_reports_its_reason_and_gets_nothing() {
        let links = Links::new(Ok(vec![Phone {
            id: "a".to_owned(),
            blocked: Some("허용 안 됨".to_owned()),
        }]));
        let outcomes = greet(&Name(Ok("mac")), &[&links]).unwrap();
        assert_eq!(outcomes[0].result, Err(PortError::new("허용 안 됨")));
        assert!(links.sent.borrow().is_empty());
    }

    #[test]
    fn a_broken_link_does_not_stop_the_other() {
        let broken = Links::new(Err(PortError::new("꺼짐")));
        let working = Links::new(Ok(vec![ready("a")]));
        let outcomes = greet(&Name(Ok("mac")), &[&broken, &working]).unwrap();
        assert_eq!(outcomes[0].target, "fake");
        assert_eq!(outcomes[0].result, Err(PortError::new("꺼짐")));
        assert_eq!(outcomes[1].result, Ok(()));
    }

    #[test]
    fn stops_without_a_name() {
        let links = Links::new(Ok(vec![ready("a")]));
        assert!(greet(&Name(Err("못 읽음")), &[&links]).is_err());
        assert!(greet(&Name(Ok("  ")), &[&links]).is_err());
        assert!(links.sent.borrow().is_empty());
    }
}

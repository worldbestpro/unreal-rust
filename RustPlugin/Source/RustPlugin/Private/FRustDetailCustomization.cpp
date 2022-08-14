#include "FRustDetailCustomization.h"

// MyCustomization.cpp
#include "PropertyEditing.h"
#include "Widgets/Input/SVectorInputBox.h"
#include "RustActor.h"
#include "RustProperty.h"
#include "SRustDropdownList.h"

#define LOCTEXT_NAMESPACE "RustDetailCustomization"

TSharedRef<IDetailCustomization> FRustDetailCustomization::MakeInstance()
{
	return MakeShareable(new FRustDetailCustomization);
}

void FRustDetailCustomization::CustomizeDetails(IDetailLayoutBuilder& DetailBuilder)
{
	TArray<TWeakObjectPtr<UObject>> Objects;
	DetailBuilder.GetObjectsBeingCustomized(Objects);


	if (Objects.IsEmpty())
		return;

	TWeakObjectPtr<UEntityComponent> Component = Cast<UEntityComponent>(Objects[0]);
	//TSharedRef<IPropertyHandle> Handle = DetailBuilder.GetProperty(
	//	GET_MEMBER_NAME_CHECKED(UEntityComponent, Components));
	TSharedRef<IPropertyHandle> ComponentsHandle = DetailBuilder.GetProperty(
		GET_MEMBER_NAME_CHECKED(UEntityComponent, Components));


	IDetailCategoryBuilder& RustCategory = DetailBuilder.EditCategory(TEXT("Rust"));
	auto OnPicked = [Component, &DetailBuilder, ComponentsHandle](FUuidViewNode* Node)
	{
		if (Node == nullptr || Component == nullptr)
			return;

		{
			ComponentsHandle->AsMap()->AddItem();
			uint32 NumChildren = 0;
			ComponentsHandle->GetNumChildren(NumChildren);
			auto ChildProp = ComponentsHandle->GetChildHandle(NumChildren - 1);
			auto KeyProp = ChildProp->GetKeyHandle();

			KeyProp->SetValue(Node->Id.ToString());
			FDynamicRustComponent::Initialize(ChildProp, Node->Id);
		}
		DetailBuilder.ForceRefreshDetails();
	};

	FDynamicRustComponent::Render(ComponentsHandle, RustCategory, DetailBuilder.GetPropertyUtilities(),
	                               FOnComponentRemoved());

	RustCategory.AddCustomRow(LOCTEXT("Picker", "Picker")).WholeRowContent()[
		SNew(SRustDropdownList).OnUuidPickedDelegate(FOnUuidPicked::CreateLambda(OnPicked))
	];
}

#undef LOCTEXT_NAMESPACE
